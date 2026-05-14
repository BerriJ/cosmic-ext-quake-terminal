use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Arc;

use clap::Parser;
use cosmic::app::{Core, Settings, Task};
use cosmic::cosmic_config::{self, CosmicConfigEntry};
use cosmic::iced::event::{self, Event};
use cosmic::iced::window;
use cosmic::iced::Length;
use cosmic::iced::Subscription;
use cosmic::widget::{container, header_bar, scrollable, settings, text, text_input};
use cosmic::{Application, ApplicationExt, Element};
use serde::{Deserialize, Serialize};

use crate::config::{QuakeConfig, CONFIG_VERSION};
use crate::fl;
use crate::process::{self, TERMINAL_APP_ID};
use crate::wayland::{self, ToplevelEvent, WaylandController};

const APP_ID: &str = "com.github.m0rf30.CosmicExtQuakeTerminal";

#[derive(Parser, Debug, Serialize, Deserialize, Clone)]
#[command(name = "cosmic-ext-quake-terminal")]
#[command(about = "Quake-style dropdown terminal for COSMIC Desktop")]
pub struct Args {
    #[command(subcommand)]
    pub subcommand: Option<QuakeAction>,
}

#[derive(Debug, Serialize, Deserialize, Clone, clap::Subcommand)]
pub enum QuakeAction {
    /// Toggle the quake terminal visibility
    Toggle,
    /// Open the settings window
    Settings,
}

impl std::fmt::Display for QuakeAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            QuakeAction::Toggle => write!(f, "Toggle"),
            QuakeAction::Settings => write!(f, "Settings"),
        }
    }
}

impl std::str::FromStr for QuakeAction {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "Toggle" => Ok(QuakeAction::Toggle),
            "Settings" => Ok(QuakeAction::Settings),
            other => Err(format!("Unknown action: {other}")),
        }
    }
}

impl cosmic::app::CosmicFlags for Args {
    type SubCommand = QuakeAction;
    type Args = Vec<String>;

    fn action(&self) -> Option<&QuakeAction> {
        self.subcommand.as_ref()
    }
}

#[derive(Debug, Clone, PartialEq)]
enum ToggleState {
    Idle,
    WaitingForWindow,
    Visible,
    Hidden,
}

#[derive(Debug, Clone)]
pub enum Message {
    Toggle,
    ToplevelEvent(ToplevelEvent),
    TerminalExited,
    ConfigChanged(QuakeConfig),
    OpenSettings,
    WindowOpened(window::Id),
    WindowClosed(window::Id),
    CloseWindow(window::Id),
    SetTerminalArgs(String),
    FocusRetry,
}

/// Number of `activate` retries we issue while trying to claim focus after a
/// show toggle. A child app launched from the terminal will often try to
/// reclaim focus immediately after the terminal becomes visible; each retry
/// gives the compositor another chance to honor our request. With the
/// 150 ms tick this gives a ~1.5 s window for focus to settle.
const MAX_FOCUS_RETRIES: u8 = 10;

pub struct QuakeTerminal {
    core: Core,
    config: QuakeConfig,
    config_handler: Option<cosmic_config::Config>,
    state: ToggleState,
    focused: bool,
    refocusing: bool,
    /// True while we are actively trying to claim keyboard focus on the
    /// terminal (between a show toggle and the moment focus has settled).
    /// While this is set, `Deactivated` events do not trigger auto-hide.
    focus_pending: bool,
    focus_retries: u8,
    terminal_pid: Option<Arc<AtomicU32>>,
    wayland_controller: Option<WaylandController>,
    settings_window_id: Option<window::Id>,
}

impl Application for QuakeTerminal {
    type Message = Message;
    type Executor = cosmic::executor::single::Executor;
    type Flags = Args;

    const APP_ID: &'static str = APP_ID;

    fn init(core: Core, flags: Self::Flags) -> (Self, Task<Self::Message>) {
        let config_handler = cosmic_config::Config::new(APP_ID, CONFIG_VERSION).ok();
        let config = config_handler
            .as_ref()
            .and_then(|h| QuakeConfig::get_entry(h).ok())
            .unwrap_or_default();

        let app = Self {
            core,
            config,
            config_handler,
            state: ToggleState::Idle,
            focused: false,
            refocusing: false,
            focus_pending: false,
            focus_retries: 0,
            terminal_pid: None,
            wayland_controller: None,
            settings_window_id: None,
        };

        // Dispatch the initial action from CLI flags (first-instance case)
        let task = match flags.subcommand {
            Some(QuakeAction::Settings) => cosmic::task::message(Message::OpenSettings),
            Some(QuakeAction::Toggle) => cosmic::task::message(Message::Toggle),
            None => Task::none(),
        };

        (app, task)
    }

    fn core(&self) -> &Core {
        &self.core
    }

    fn core_mut(&mut self) -> &mut Core {
        &mut self.core
    }

    fn update(&mut self, message: Self::Message) -> Task<Self::Message> {
        match message {
            Message::Toggle => self.handle_toggle(),
            Message::ToplevelEvent(event) => self.handle_toplevel_event(event),
            Message::TerminalExited => {
                // Only reap the zombie process — do NOT reset state.
                // Many terminals fork (parent exits, child keeps running),
                // so PID death does not mean the window is gone.
                // State is driven by ToplevelEvent::Closed instead.
                tracing::info!("Terminal process exited (reaping zombie)");
                if let Some(pid) = self.terminal_pid.take() {
                    let raw = pid.load(Ordering::Relaxed) as i32;
                    let _ = nix::sys::wait::waitpid(
                        nix::unistd::Pid::from_raw(raw),
                        Some(nix::sys::wait::WaitPidFlag::WNOHANG),
                    );
                }
            }
            Message::ConfigChanged(config) => {
                tracing::info!("Config changed");
                self.config = config;
            }
            Message::OpenSettings => {
                if self.settings_window_id.is_some() {
                    return Task::none();
                }
                let settings = window::Settings {
                    size: cosmic::iced::Size::new(500.0, 450.0),
                    resizable: true,
                    decorations: false,
                    ..window::Settings::default()
                };
                let (id, task) = window::open(settings);
                self.settings_window_id = Some(id);
                let title = fl!("settings-title");
                return task.discard().chain(self.set_window_title(title, id));
            }
            Message::WindowOpened(_id) => {}
            Message::CloseWindow(id) => {
                if self.settings_window_id == Some(id) {
                    self.settings_window_id = None;
                    return window::close(id);
                }
            }
            Message::WindowClosed(id) => {
                if self.settings_window_id == Some(id) {
                    self.settings_window_id = None;
                }
            }
            Message::SetTerminalArgs(args_str) => {
                let args: Vec<String> = if args_str.trim().is_empty() {
                    Vec::new()
                } else {
                    args_str.split_whitespace().map(String::from).collect()
                };
                if let Some(ref handler) = self.config_handler {
                    let _ = self.config.set_terminal_args(handler, args);
                }
            }
            Message::FocusRetry => self.handle_focus_retry(),
        }
        Task::none()
    }

    fn view(&self) -> Element<'_, Self::Message> {
        // Daemon mode - no main window
        text("").into()
    }

    fn view_window(&self, id: window::Id) -> Element<'_, Self::Message> {
        if self.settings_window_id != Some(id) {
            return text("").into();
        }

        let terminal_section = settings::section().title(fl!("settings-terminal")).add(
            settings::item(
                fl!("terminal-args"),
                text_input(
                    fl!("terminal-args-placeholder"),
                    self.config.terminal_args.join(" "),
                )
                .on_input(Message::SetTerminalArgs),
            ),
        );

        let content = settings::view_column(vec![terminal_section.into()]).padding([0, 24]);

        let header = header_bar()
            .title(fl!("settings-title"))
            .on_close(Message::CloseWindow(id));

        container(cosmic::widget::column().push(header).push(scrollable(content)))
            .class(cosmic::style::Container::Background)
            .width(Length::Fill)
            .height(Length::Fill)
            .into()
    }

    fn subscription(&self) -> Subscription<Self::Message> {
        let mut subs = vec![wayland::toplevel_subscription(TERMINAL_APP_ID)
            .map(Message::ToplevelEvent)];

        // Monitor terminal process exit via kill(pid, 0)
        if let Some(ref pid_holder) = self.terminal_pid {
            let pid_holder = pid_holder.clone();
            subs.push(cosmic::iced::Subscription::run_with_id(
                "process-monitor",
                futures::stream::unfold(pid_holder, |pid_holder| async move {
                    loop {
                        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
                        let pid = pid_holder.load(Ordering::Relaxed);
                        if pid != 0 {
                            let alive = nix::sys::signal::kill(
                                nix::unistd::Pid::from_raw(pid as i32),
                                None,
                            )
                            .is_ok();
                            if !alive {
                                return Some((Message::TerminalExited, pid_holder));
                            }
                        }
                    }
                }),
            ));
        }

        // Focus-retry ticker: while we're trying to claim focus after a
        // show toggle, fire a FocusRetry message every 150ms so the update
        // loop can re-issue activate until focus actually lands on us.
        if self.focus_pending {
            subs.push(cosmic::iced::Subscription::run_with_id(
                "focus-retry",
                futures::stream::unfold((), |()| async move {
                    tokio::time::sleep(std::time::Duration::from_millis(150)).await;
                    Some((Message::FocusRetry, ()))
                }),
            ));
        }

        // Watch for config changes
        if self.config_handler.is_some() {
            subs.push(
                cosmic_config::config_subscription::<_, QuakeConfig>(
                    std::any::TypeId::of::<QuakeConfig>(),
                    APP_ID.into(),
                    CONFIG_VERSION,
                )
                .map(|update| {
                    if !update.errors.is_empty() {
                        tracing::warn!("Config errors: {:?}", update.errors);
                    }
                    Message::ConfigChanged(update.config)
                }),
            );
        }

        // Watch for window events (settings window open/close)
        subs.push(event::listen_with(|event, _status, id| match event {
            Event::Window(window::Event::CloseRequested) => Some(Message::CloseWindow(id)),
            Event::Window(window::Event::Opened { .. }) => Some(Message::WindowOpened(id)),
            Event::Window(window::Event::Closed) => Some(Message::WindowClosed(id)),
            _ => None,
        }));

        Subscription::batch(subs)
    }

    fn dbus_activation(&mut self, msg: cosmic::dbus_activation::Message) -> Task<Self::Message> {
        use cosmic::dbus_activation::Details;

        match msg.msg {
            Details::Activate => {
                return cosmic::task::message(Message::Toggle);
            }
            Details::ActivateAction { action, .. } => {
                if let Ok(cmd) = action.parse::<QuakeAction>() {
                    match cmd {
                        QuakeAction::Toggle => {
                            return cosmic::task::message(Message::Toggle);
                        }
                        QuakeAction::Settings => {
                            return cosmic::task::message(Message::OpenSettings);
                        }
                    }
                }
            }
            Details::Open { .. } => {}
        }
        Task::none()
    }
}

impl QuakeTerminal {
    fn handle_toggle(&mut self) {
        match self.state {
            ToggleState::Idle => {
                tracing::info!("Toggle: spawning terminal");
                let result = process::spawn_terminal(&self.config.terminal_args);
                if let Some(result) = result {
                    let pid = result.pid;
                    self.terminal_pid = Some(Arc::new(AtomicU32::new(pid)));
                    self.state = ToggleState::WaitingForWindow;
                }
            }
            ToggleState::WaitingForWindow => {
                tracing::debug!("Toggle: still waiting for window to appear");
            }
            ToggleState::Visible => {
                if self.focused {
                    tracing::info!("Toggle: hiding terminal");
                    if let Some(ref controller) = self.wayland_controller {
                        controller.minimize();
                    }
                    self.state = ToggleState::Hidden;
                    self.focused = false;
                    // User explicitly hid the terminal — stop any pending
                    // focus-retry loop that may still be running.
                    self.focus_pending = false;
                    self.focus_retries = 0;
                } else {
                    tracing::info!("Toggle: refocusing terminal (minimize first)");
                    if let Some(ref controller) = self.wayland_controller {
                        controller.minimize();
                    }
                    self.refocusing = true;
                    // Arm the focus-retry loop so that if the launched child
                    // app reclaims focus right after we reactivate, we keep
                    // trying instead of letting auto-hide fire.
                    self.focus_pending = true;
                    self.focus_retries = 0;
                }
            }
            ToggleState::Hidden => {
                tracing::info!("Toggle: showing terminal");
                // Arm focus-retry BEFORE sending activate. The compositor may
                // briefly activate then deactivate the terminal as a child
                // app reclaims focus; the retry loop (driven by the
                // focus-retry subscription) will keep re-issuing activate
                // until focus actually settles on us or we hit the retry cap.
                self.focus_pending = true;
                self.focus_retries = 0;
                if let Some(ref controller) = self.wayland_controller {
                    controller.activate();
                }
                self.state = ToggleState::Visible;
                // Do NOT optimistically set focused=true here: we want the
                // retry loop to keep firing until the real Activated event
                // arrives from the compositor.
            }
        }
    }

    /// Driven by the `focus-retry` subscription. Re-issues an `activate`
    /// request while we still believe focus has not landed on the terminal.
    /// This is what defeats the race where a child app launched from the
    /// terminal reclaims focus immediately after we activate.
    fn handle_focus_retry(&mut self) {
        if !self.focus_pending {
            return;
        }
        if self.focus_retries >= MAX_FOCUS_RETRIES {
            // Retry budget exhausted. Stop the retry loop. If focus never
            // landed, the next user toggle will try again.
            tracing::debug!(
                "Focus retry: budget exhausted after {} attempts (focused={})",
                self.focus_retries,
                self.focused
            );
            self.focus_pending = false;
            self.focus_retries = 0;
            return;
        }
        self.focus_retries += 1;
        // Always re-issue activate if we don't currently hold focus,
        // even if a previous `Activated` event already arrived: the
        // compositor may have bounced focus back to the previously-focused
        // app, and we want to claim it back. If we DO currently hold focus
        // the activate is a cheap no-op.
        if !self.focused {
            tracing::debug!(
                "Focus retry: attempt {} (not focused)",
                self.focus_retries
            );
            if let Some(ref controller) = self.wayland_controller {
                controller.activate();
            }
        } else {
            tracing::trace!(
                "Focus retry: attempt {} (already focused, holding)",
                self.focus_retries
            );
        }
    }

    fn handle_toplevel_event(&mut self, event: ToplevelEvent) {
        match event {
            ToplevelEvent::Ready(controller) => {
                tracing::info!("Wayland toplevel controller ready");
                self.wayland_controller = Some(controller);
            }
            ToplevelEvent::Found => {
                tracing::info!("Terminal window found");
                if self.state == ToggleState::WaitingForWindow {
                    self.state = ToggleState::Visible;
                    self.focused = true;
                }
            }
            ToplevelEvent::Minimized => {
                if self.terminal_pid.is_some() {
                    if self.refocusing {
                        // Compositor confirmed minimize — now activate to bring to front
                        tracing::info!("Refocus: minimize confirmed, activating");
                        self.refocusing = false;
                        if let Some(ref controller) = self.wayland_controller {
                            controller.activate();
                        }
                        // Leave focused=false: the Activated event will set it
                        // truthfully and clear focus_pending. Setting it
                        // optimistically here would short-circuit the retry
                        // loop before focus actually lands on the terminal.
                    } else {
                        self.state = ToggleState::Hidden;
                        self.focused = false;
                    }
                }
            }
            ToplevelEvent::Activated => {
                if self.terminal_pid.is_some() {
                    self.state = ToggleState::Visible;
                    self.focused = true;
                    // NOTE: we deliberately do NOT clear `focus_pending`
                    // here. After a show toggle the compositor sometimes
                    // briefly activates us and then bounces focus back to
                    // the previously-focused app (the one launched from
                    // the terminal). Keeping `focus_pending` armed for the
                    // full retry budget lets the retry tick re-issue
                    // `activate` until focus actually sticks, and keeps
                    // `Deactivated` from triggering auto-hide in the
                    // meantime. The retry tick clears `focus_pending`
                    // once the budget is exhausted.
                }
            }
            ToplevelEvent::Deactivated => {
                if self.terminal_pid.is_some() {
                    self.focused = false;
                    // While the retry loop is active, do NOT auto-hide. The
                    // compositor may briefly deactivate us as a child app
                    // (launched from the terminal) reclaims focus; the
                    // retry tick will re-issue activate.
                    if self.focus_pending {
                        return;
                    }
                    // Auto-hide when the terminal loses focus (e.g. user
                    // launched an app from it and that app stole focus).
                    // Skip while we're intentionally cycling focus via the
                    // refocus flow.
                    if self.state == ToggleState::Visible && !self.refocusing {
                        tracing::info!("Auto-hide: terminal lost focus, minimizing");
                        if let Some(ref controller) = self.wayland_controller {
                            controller.minimize();
                        }
                        self.state = ToggleState::Hidden;
                    }
                }
            }
            ToplevelEvent::Closed => {
                tracing::info!("Terminal window closed by compositor");
                self.state = ToggleState::Idle;
                self.focused = false;
                self.focus_pending = false;
                self.focus_retries = 0;
                if let Some(pid) = self.terminal_pid.take() {
                    let raw = pid.load(Ordering::Relaxed) as i32;
                    let nix_pid = nix::unistd::Pid::from_raw(raw);
                    let _ = nix::sys::signal::kill(nix_pid, nix::sys::signal::Signal::SIGTERM);
                    let _ = nix::sys::wait::waitpid(
                        nix_pid,
                        Some(nix::sys::wait::WaitPidFlag::WNOHANG),
                    );
                }
            }
        }
    }
}

pub fn run() -> cosmic::iced::Result {
    let args = Args::parse();

    cosmic::app::run_single_instance::<QuakeTerminal>(
        Settings::default()
            .no_main_window(true)
            .exit_on_close(false),
        args,
    )
}
