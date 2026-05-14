{
  self,
  lib,
  openssl,
  git,
  cmake,
  rustPlatform,
  libxkbcommon,
  pkg-config,
  glib,
  gtk3,
  webkitgtk_4_1,
  wayland,
  makeBinaryWrapper,
}:

rustPlatform.buildRustPackage rec {
  pname = "quake";
  version = "0.1.0";

  src = self;

  cargoHash = "sha256-846q7q1Rt2z5qkGK1+IHazzuvR8i8IeU0vK/cWn2vwI=";

  buildInputs = [
    wayland
    glib
    libxkbcommon
    pkg-config
    openssl
    git
    gtk3
    webkitgtk_4_1
  ];
  nativeBuildInputs = [
    cmake
    pkg-config
    makeBinaryWrapper
  ];

  postInstall = ''
    wrapProgram $out/bin/cosmic-ext-quake-terminal \
      --prefix LD_LIBRARY_PATH : ${
        lib.makeLibraryPath [
          wayland
          libxkbcommon
        ]
      }
  '';

  meta = {
    description = "A quake-style dropdown terminal for COSMIC Desktop.";
    homepage = "https://github.com/M0Rf30/cosmic-ext-quake-terminal/tags";
    license = lib.licenses.gpl3Only;
    maintainers = with lib.maintainers; [ berrij ];
  };
}
