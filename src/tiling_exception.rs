//! Ensure cosmic-comp treats the spawned terminal as floating.
//!
//! cosmic-comp reads window rules from the cosmic-config namespace
//! `com.system76.CosmicSettings.WindowRules` (v1). When the user has enabled
//! the tiling layout, new toplevels are tiled by default; to keep the Quake
//! dropdown behaviour, we need an entry in `tiling_exception_custom` matching
//! the terminal's app_id. This module makes sure such an entry exists so the
//! user does not have to configure it manually.
//!
//! The config schema (mirrored from cosmic-settings-config) uses regex
//! matching on both `appid` and `title`; an empty `title` regex matches any
//! title for the given app_id.

use cosmic_config::{Config, ConfigGet, ConfigSet};
use serde::{Deserialize, Serialize};

const WINDOW_RULES_ID: &str = "com.system76.CosmicSettings.WindowRules";
const WINDOW_RULES_VERSION: u64 = 1;
const CUSTOM_KEY: &str = "tiling_exception_custom";

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
struct PreciseApplicationException {
    appid: String,
    title: String,
    enabled: bool,
}

/// Make sure `cosmic-comp` has a tiling exception registered for the given
/// `app_id`, so the spawned terminal window opens floating even with tiling
/// enabled. This is idempotent and safe to call on every startup.
pub fn ensure(app_id: &str) {
    let config = match Config::new(WINDOW_RULES_ID, WINDOW_RULES_VERSION) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!(
                "Could not open cosmic-comp window rules config ({WINDOW_RULES_ID}): {e}"
            );
            return;
        }
    };

    let mut custom: Vec<PreciseApplicationException> = config
        .get_local(CUSTOM_KEY)
        .or_else(|_| config.get(CUSTOM_KEY))
        .unwrap_or_default();

    // If an enabled entry for this app_id already exists (any title), do nothing.
    if custom
        .iter()
        .any(|e| e.enabled && e.appid == app_id && e.title.is_empty())
    {
        tracing::debug!("Tiling exception for {app_id} already present");
        return;
    }

    // If a disabled entry exists for the same (appid, ""), re-enable it
    // instead of duplicating.
    if let Some(existing) = custom
        .iter_mut()
        .find(|e| e.appid == app_id && e.title.is_empty())
    {
        existing.enabled = true;
    } else {
        custom.push(PreciseApplicationException {
            appid: app_id.to_string(),
            title: String::new(),
            enabled: true,
        });
    }

    match config.set(CUSTOM_KEY, custom) {
        Ok(()) => tracing::info!(
            "Registered cosmic-comp tiling exception for {app_id} (window will open floating)"
        ),
        Err(e) => tracing::warn!("Failed to register tiling exception for {app_id}: {e}"),
    }
}
