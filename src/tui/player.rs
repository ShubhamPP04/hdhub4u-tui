use crate::tui::state::PlayerKind;
use std::{path::Path, process::Command};

const MPV_WINDOWS: &str = r"C:\Program Files\mpv\mpv.exe";
const MPV_MACOS: &str = "/Applications/mpv.app/Contents/MacOS/mpv";
const VLC_WINDOWS: &str = r"C:\Program Files\VideoLAN\VLC\vlc.exe";
const VLC_WINDOWS_X86: &str = r"C:\Program Files (x86)\VideoLAN\VLC\vlc.exe";
const VLC_MACOS: &str = "/Applications/VLC.app/Contents/MacOS/VLC";

pub fn detect() -> Vec<PlayerKind> {
    let mut players = Vec::new();

    #[cfg(target_os = "macos")]
    if iina_available() {
        players.push(PlayerKind::Iina);
    }

    if mpv_executable().is_some() {
        players.push(PlayerKind::Mpv);
    }
    if vlc_executable().is_some() {
        players.push(PlayerKind::Vlc);
    }

    players
}

pub fn supports_headers(kind: PlayerKind, headers: &[(String, String)]) -> bool {
    kind != PlayerKind::Vlc
        || headers.iter().all(|(name, _)| {
            name.eq_ignore_ascii_case("referer") || name.eq_ignore_ascii_case("user-agent")
        })
}

pub fn command(
    kind: PlayerKind,
    url: &str,
    subtitle: Option<&str>,
    headers: &[(String, String)],
) -> Command {
    match kind {
        PlayerKind::Mpv => mpv_command(url, subtitle, headers, false),
        PlayerKind::Iina => iina_command(url, subtitle, headers),
        PlayerKind::Vlc => vlc_command(url, subtitle, headers),
    }
}

fn mpv_command(
    url: &str,
    subtitle: Option<&str>,
    headers: &[(String, String)],
    iina: bool,
) -> Command {
    let fallback = if cfg!(target_os = "windows") {
        "mpv.exe"
    } else {
        "mpv"
    };
    let executable = mpv_executable().unwrap_or_else(|| fallback.into());
    let mut command = if executable.starts_with("flatpak run ") {
        let mut parts = executable.split(' ');
        let mut cmd = Command::new(parts.next().unwrap());
        cmd.args(parts);
        cmd
    } else {
        Command::new(executable)
    };
    let prefix = if iina { "--mpv-" } else { "--" };

    command
        .arg(format!("{prefix}autofit=960x540"))
        .arg(format!("{prefix}autofit-larger=640x360"))
        .arg(format!("{prefix}geometry=50%:50%"))
        .arg(url);

    if !headers.is_empty() {
        let fields = headers
            .iter()
            .map(|(name, value)| format!("{name}: {value}"))
            .collect::<Vec<_>>()
            .join(",");
        command.arg(format!("{prefix}http-header-fields={fields}"));
    }
    if let Some(subtitle) = subtitle {
        if iina {
            command.arg(format!("--mpv-sub-files={subtitle}"));
        } else {
            command.arg(format!("--sub-file={subtitle}"));
        }
    }

    command
}

#[cfg(target_os = "macos")]
fn iina_command(url: &str, subtitle: Option<&str>, headers: &[(String, String)]) -> Command {
    let configured = configured_executable("MOVIEBOX_IINA_PATH");
    let mut command = if let Some(executable) = configured {
        Command::new(executable)
    } else if iina_app_exists() {
        let mut c = Command::new("open");
        c.arg("-a").arg("IINA").arg("--args");
        c
    } else {
        Command::new("iina")
    };
    let mpv = mpv_command(url, subtitle, headers, true);
    command.args(mpv.get_args());
    command
}

#[cfg(not(target_os = "macos"))]
fn iina_command(url: &str, subtitle: Option<&str>, headers: &[(String, String)]) -> Command {
    mpv_command(url, subtitle, headers, false)
}

fn vlc_command(url: &str, subtitle: Option<&str>, headers: &[(String, String)]) -> Command {
    let fallback = if cfg!(target_os = "windows") {
        "vlc.exe"
    } else {
        "vlc"
    };
    let executable = vlc_executable().unwrap_or_else(|| fallback.into());
    let mut command = if executable.starts_with("flatpak run ") {
        let mut parts = executable.split(' ');
        let mut cmd = Command::new(parts.next().unwrap());
        cmd.args(parts);
        cmd
    } else {
        Command::new(executable)
    };
    command.arg("--width=960").arg("--height=540").arg(url);

    for (name, value) in headers {
        if name.eq_ignore_ascii_case("referer") {
            command.arg(format!("--http-referrer={value}"));
        } else if name.eq_ignore_ascii_case("user-agent") {
            command.arg(format!("--http-user-agent={value}"));
        }
    }
    if let Some(subtitle) = subtitle {
        command.arg(format!("--sub-file={subtitle}"));
    }

    command
}

fn mpv_executable() -> Option<String> {
    if let Some(executable) = configured_executable("MOVIEBOX_MPV_PATH") {
        return Some(executable);
    }
    let fallback = if cfg!(target_os = "windows") {
        "mpv.exe"
    } else {
        "mpv"
    };
    #[allow(unused_mut)]
    let mut paths = vec![MPV_WINDOWS.to_string(), MPV_MACOS.to_string()];
    #[cfg(target_os = "macos")]
    if let Some(home) = dirs::home_dir() {
        paths.push(
            home.join("Applications/mpv.app/Contents/MacOS/mpv")
                .to_string_lossy()
                .into_owned(),
        );
    }
    #[cfg(windows)]
    if let Ok(local) = std::env::var("LOCALAPPDATA") {
        paths.push(format!(r"{local}\Programs\mpv\mpv.exe"));
    }
    first_executable(&paths, fallback).or_else(|| flatpak_executable("io.mpv.Mpv"))
}

fn vlc_executable() -> Option<String> {
    if let Some(executable) = configured_executable("MOVIEBOX_VLC_PATH") {
        return Some(executable);
    }
    let fallback = if cfg!(target_os = "windows") {
        "vlc.exe"
    } else {
        "vlc"
    };

    #[allow(unused_mut)]
    let mut paths = vec![
        VLC_WINDOWS.to_string(),
        VLC_WINDOWS_X86.to_string(),
        VLC_MACOS.to_string(),
    ];

    #[cfg(target_os = "macos")]
    if let Some(home) = dirs::home_dir() {
        paths.push(
            home.join("Applications/VLC.app/Contents/MacOS/VLC")
                .to_string_lossy()
                .into_owned(),
        );
    }

    #[cfg(windows)]
    let windows_app_path = std::env::var("LOCALAPPDATA")
        .map(|l| format!(r"{}\Microsoft\WindowsApps\vlc.exe", l))
        .unwrap_or_default();

    #[cfg(windows)]
    if !windows_app_path.is_empty() {
        paths.push(windows_app_path);
    }

    first_executable(&paths, fallback).or_else(|| flatpak_executable("org.videolan.VLC"))
}

#[cfg(target_os = "macos")]
fn iina_available() -> bool {
    configured_executable("MOVIEBOX_IINA_PATH").is_some()
        || iina_app_exists()
        || command_exists("iina")
}

#[cfg(target_os = "macos")]
fn iina_app_exists() -> bool {
    Path::new("/Applications/IINA.app").exists()
        || dirs::home_dir().is_some_and(|home| home.join("Applications/IINA.app").exists())
}

fn flatpak_executable(app_id: &str) -> Option<String> {
    if !cfg!(target_os = "linux") {
        return None;
    }
    if command_exists("flatpak") {
        let mut cmd = Command::new("flatpak");
        cmd.arg("info")
            .arg(app_id)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null());
        if cmd.output().map(|o| o.status.success()).unwrap_or(false) {
            return Some(format!("flatpak run {}", app_id));
        }
    }
    None
}

fn first_executable(paths: &[String], fallback: &str) -> Option<String> {
    paths
        .iter()
        .find(|path| Path::new(path).exists())
        .cloned()
        .or_else(|| command_exists(fallback).then(|| fallback.to_string()))
}

fn configured_executable(variable: &str) -> Option<String> {
    std::env::var(variable)
        .ok()
        .filter(|value| !value.trim().is_empty())
        .filter(|value| Path::new(value).exists() || command_exists(value))
}

fn command_exists(command: &str) -> bool {
    let mut cmd = Command::new(command);
    cmd.arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x08000000);
    }
    cmd.output().is_ok_and(|output| output.status.success())
}
