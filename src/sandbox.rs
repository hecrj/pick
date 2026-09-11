//! Sandboxing the process with bubblewrap.
//!
//! The model runs arbitrary commands through the `bash` tool and may
//! pass absolute paths to the file tools, so before the runtime
//! starts, [`enter`] re-executes the process under a bubblewrap
//! sandbox:
//!
//! - the user, pid, ipc, and uts namespaces are unshared, so the
//!   sandboxed process has no host identity and cannot see or signal
//!   host processes;
//! - the root filesystem is bound read-only, with the project and
//!   the user's projects directory as the writable places besides
//!   the ssh-agent socket, and `/tmp` a private tmpfs;
//! - the home directory is replaced by a tmpfs, hiding credentials
//!   and configuration, with only the Rust toolchain remounted;
//! - the user's projects directory is bound read-write, so the
//!   model can work across sibling projects, not just the one it
//!   was launched in;
//! - the GPU's device nodes are remounted on the minimal `/dev`,
//!   and the X11 socket directory is bound into the private `/tmp`,
//!   so the renderer can use the hardware instead of the CPU;
//! - the ssh-agent socket is bound in, and the directory it lives
//!   in blanked, so the GnuPG agent's other sockets — the full
//!   gpg-agent protocol, with its secret-key commands — stay out
//!   while ssh transport and signing keep working; the host's
//!   known ssh host keys come back read-only, so `git` can reach
//!   private repositories without the key material ever entering
//!   the sandbox;
//! - the ssh signing public key and the allowed-signers file the
//!   global git configuration names are bound read-only, so
//!   `gpg.format = ssh` commits can be made and verified
//!   in-sandbox; the private key they reference stays behind the
//!   home tmpfs;
//! - the environment is cleared down to `PATH`, `HOME`, `USER`, the
//!   ssh-agent socket variable, the display variables the GUI needs
//!   to reach the compositor, and the host's locale variables —
//!   `LC_ALL`, any `LC_*`, `LANG`, or `LC_ALL=C.UTF-8` where the
//!   host set none — so gpg and git render accented names instead
//!   of mangling them under the bare C locale, while tokens the
//!   user exported never reach the model's shell.
//!
//! [`enter`] is called once from `main`. When the re-execution
//! succeeds it does not return: the process image is replaced, and
//! the new process re-enters `main` with [`MARKER`] set, where it
//! becomes a no-op. When a sandbox cannot be started, it reports
//! why, so `main` can panic rather than keep running bare; the
//! `--we-doin-it-live` flag skips the sandbox entirely.

use std::env;
use std::ffi::OsString;
use std::fmt;
use std::path::{Path, PathBuf};

#[cfg(target_os = "linux")]
use std::os::unix::process::CommandExt;

/// The environment marker identifying an already-sandboxed process,
/// so the re-executed process does not sandbox itself again.
const MARKER: &str = "PICK_SANDBOXED";
/// The sandbox hostname, in the unshared uts namespace.
const HOSTNAME: &str = "pick";
/// The bubblewrap binary, looked up on the `PATH`.
const BINARY: &str = "bwrap";

/// Re-executes the process under a bubblewrap sandbox in which
/// `project`, the directory the tools operate in, and the user's
/// projects directory are the only writable places.
///
/// See the module documentation for the policy.
#[cfg_attr(not(target_os = "linux"), allow(unused_variables))]
pub fn enter(project: &Path) -> Status {
    if env::var_os(MARKER).is_some() {
        return Status::Sandboxed;
    }

    #[cfg(target_os = "linux")]
    {
        enter_linux(project)
    }

    #[cfg(not(target_os = "linux"))]
    {
        Status::Bare(Reason::Unsupported)
    }
}

/// Whether the process runs sandboxed, and why not.
pub enum Status {
    /// The process runs inside the bubblewrap sandbox.
    Sandboxed,
    /// The process runs bare; the reason is why the sandbox could
    /// not be started.
    Bare(Reason),
}

/// Why a process runs without a sandbox.
pub enum Reason {
    /// Bubblewrap only exists on Linux.
    #[cfg_attr(target_os = "linux", allow(dead_code))]
    Unsupported,
    /// No `bwrap` binary was found on the `PATH`.
    Missing,
    /// The probe failed; the detail says why, typically user
    /// namespaces being disabled.
    Probe(String),
    /// The project directory could not be resolved.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Project,
    /// The re-execution failed for some other reason.
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    Exec(String),
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Unsupported => f.write_str("bubblewrap is only available on Linux"),
            Self::Missing => f.write_str("no `bwrap` binary on the PATH; install bubblewrap"),
            Self::Probe(detail) => {
                if detail.trim().is_empty() {
                    f.write_str("the sandbox probe failed (user namespaces may be disabled)")
                } else {
                    write!(f, "the sandbox probe failed: {detail}")
                }
            }
            Self::Project => f.write_str("the project directory could not be resolved"),
            Self::Exec(detail) => write!(f, "the sandbox re-execution failed: {detail}"),
        }
    }
}

#[cfg(target_os = "linux")]
fn enter_linux(project: &Path) -> Status {
    let project = match project.canonicalize() {
        Ok(project) => project,
        Err(_) => return Status::Bare(Reason::Project),
    };

    let bwrap = match find_on_path(BINARY, env::var_os("PATH").as_deref()) {
        Some(bwrap) => bwrap,
        None => return Status::Bare(Reason::Missing),
    };

    if let Err(detail) = probe(&bwrap) {
        return Status::Bare(Reason::Probe(detail));
    }

    let exe = match env::current_exe() {
        Ok(exe) => exe,
        Err(error) => return Status::Bare(Reason::Exec(error.to_string())),
    };

    let mut args = bwrap_args(&project, env::home_dir().as_deref(), &exe);

    // Forward the original arguments, like the initial prompt.
    args.extend(env::args_os().skip(1));

    let mut command = std::process::Command::new(&bwrap);
    command.args(&args);

    // `exec` only returns on failure; on success it replaces the
    // process image.
    let error = command.exec();
    Status::Bare(Reason::Exec(error.to_string()))
}

/// Checks that the namespaces the sandbox needs can actually be
/// created, which fails when user namespaces are disabled.
#[cfg(target_os = "linux")]
fn probe(bwrap: &Path) -> Result<(), String> {
    let output = std::process::Command::new(bwrap)
        .args([
            "--unshare-user",
            "--unshare-pid",
            "--ro-bind",
            "/",
            "/",
            "--dev",
            "/dev",
            "--proc",
            "/proc",
            "--",
            "true",
        ])
        .output()
        .map_err(|error| error.to_string())?;

    if output.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&output.stderr).trim().to_owned())
    }
}

/// Finds an executable on a `PATH` value, the way `execvp` would.
fn find_on_path(name: &str, pathvar: Option<&std::ffi::OsStr>) -> Option<PathBuf> {
    let pathvar = pathvar?;

    env::split_paths(pathvar)
        .map(|dir| dir.join(name))
        .find(|candidate| candidate.is_file())
}

/// The bubblewrap arguments that sandbox a re-execution of `exe`
/// with `project` and the user's projects directory as the only
/// writable directories.
///
/// Bubblewrap applies the mounts in the order given, so the
/// read-only root comes first and the exceptions follow.
fn bwrap_args(project: &Path, home: Option<&Path>, exe: &Path) -> Vec<OsString> {
    let os = |path: &Path| path.as_os_str().to_os_string();

    let mut args = vec![
        // Isolate the process from the host.
        OsString::from("--unshare-user"),
        OsString::from("--unshare-pid"),
        OsString::from("--unshare-ipc"),
        OsString::from("--unshare-uts"),
        // Bind the whole root filesystem read-only, then carve out
        // the writable and hidden places.
        OsString::from("--ro-bind"),
        os(Path::new("/")),
        os(Path::new("/")),
        OsString::from("--tmpfs"),
        os(Path::new("/tmp")),
        OsString::from("--tmpfs"),
        os(Path::new("/var/tmp")),
    ];

    if let Some(home) = home {
        // Replace the home directory by an empty tmpfs so nothing
        // in it — credentials, configuration, other projects — is
        // visible, and remount only what building needs.
        args.extend([OsString::from("--tmpfs"), os(home)]);

        // Cargo needs a writable registry, so `~/.cargo` comes back
        // writable; `~/.rustup` is never written to by a build, so
        // it stays read-only.
        let cargo_home = home.join(".cargo");
        if cargo_home.is_dir() {
            args.extend([OsString::from("--bind"), os(&cargo_home), os(&cargo_home)]);
        }

        let rustup_home = home.join(".rustup");
        if rustup_home.is_dir() {
            args.extend([
                OsString::from("--ro-bind"),
                os(&rustup_home),
                os(&rustup_home),
            ]);
        }

        // The user's projects directory, writable, so the model
        // can work across sibling projects, not just the one it
        // was launched in.
        let projects_home = home.join("projects");
        if projects_home.is_dir() {
            args.extend([
                OsString::from("--bind"),
                os(&projects_home),
                os(&projects_home),
            ]);
        }

        // Keep the executable reachable if it lives under the hidden
        // home and outside the project, like `~/.local/bin/pick`.
        if let Some(dir) = exe.parent()
            && dir.starts_with(home)
            && !dir.starts_with(project)
        {
            args.extend([OsString::from("--ro-bind"), os(dir), os(dir)]);
        }
    }

    // The X11 socket directory, where the X server — XWayland in a
    // Wayland session — listens. The GPU drivers reach the server
    // from the `DISPLAY` variable and fail to initialize without
    // it, which leaves the renderer without an adapter and on the
    // CPU. The private `/tmp` above hides it, so it comes back as
    // a read-only bind.
    let x11_unix = Path::new("/tmp/.X11-unix");
    if x11_unix.is_dir() {
        args.extend([OsString::from("--ro-bind"), os(x11_unix), os(x11_unix)]);
    }

    // The X authorization file, the way the X client libraries
    // find it: the `XAUTHORITY` variable, or `$HOME/.Xauthority`
    // when it is unset. It usually lives in the hidden home, so
    // the file itself is bound; the variable is forwarded with the
    // display variables below.
    if let Some(authority) = xauthority(home)
        && authority.is_file()
    {
        args.extend([OsString::from("--ro-bind"), os(&authority), os(&authority)]);
    }

    // The host's known ssh host keys, read-only, so a
    // non-interactive `git clone` can verify the host it reaches.
    // They are public data, not a credential; read-only, the model
    // can read them but not plant a hostile host key.
    if let Some(home) = home {
        let known_hosts = home.join(".ssh").join("known_hosts");
        if known_hosts.is_file() {
            args.extend([
                OsString::from("--ro-bind"),
                os(&known_hosts),
                os(&known_hosts),
            ]);
        }
    }

    // The host's global git configuration, read-only, so commits
    // in the sandbox carry the host's identity instead of dying
    // with "unable to auto-detect email address". Git, unlike ssh,
    // performs no ownership check on its config files, so the bind
    // works as-is. It is configuration, not a credential: the model
    // can read aliases and helper names but cannot alter the file,
    // and anything it references — includes, credential stores —
    // stays hidden behind the home tmpfs.
    if let Some(home) = home {
        let gitconfig = home.join(".gitconfig");
        if gitconfig.is_file() {
            args.extend([OsString::from("--ro-bind"), os(&gitconfig), os(&gitconfig)]);
        }
    }

    // The files ssh-format signing needs, as named by the global
    // git configuration, read-only. With `gpg.format = ssh`, `git`
    // reads the public key file (`user.signingkey`) to ask the agent
    // to sign, and verifies against the signers list
    // (`gpg.ssh.allowedSignersFile`); both are public data, never a
    // credential, and the private key they describe stays behind the
    // home tmpfs. A fingerprint in `user.signingkey` — the openpgp
    // form — names no file, so nothing is bound for it.
    if let Some(home) = home {
        args.extend(ssh_signing_binds(home));
    }

    // ssh(1) refuses config files owned by anyone but root or the
    // invoking user, and dies. In the rootless user namespace the
    // host's root-owned files appear owned by the unmapped uid, so
    // every system drop-in in /etc/ssh/ssh_config.d would abort the
    // first ssh invocation with "Bad owner or permissions". The
    // directory only carries distro proxy-command defaults for
    // systemd's machine/* and unix/* hosts, which the sandbox can
    // never reach anyway, so hide it behind an empty tmpfs.
    args.extend([
        OsString::from("--tmpfs"),
        os(Path::new("/etc/ssh/ssh_config.d")),
    ]);

    // A read-only root bind does not shut out sockets: connect()
    // needs write permission on the socket file's mode bits, not a
    // writable mount, so the GnuPG agent's other sockets in this
    // directory — the full gpg-agent protocol, with its secret-key
    // commands — would stay reachable too. The sandbox needs only
    // the ssh-agent half, so blank the directory and bring back
    // just that socket. `--tmpfs` takes a single destination
    // argument, unlike the `--bind` forms, so the directory is
    // named once: a second copy would be read as the command to
    // exec, failing the start with "execvp <dir>: Permission
    // denied".
    if let Some(socket) = agent_socket()
        && let Some(dir) = socket.parent()
        && dir != Path::new("/")
    {
        args.extend([OsString::from("--tmpfs"), os(dir)]);
    }

    // The host's ssh-agent socket, writable, so `git` can ask the
    // agent to sign with the host's key without the key material
    // ever entering the sandbox: a unix client needs write access
    // to the socket file to connect, and the containing directory
    // stays read-only through the root bind, so the socket cannot
    // be replaced from inside. Bubblewrap creates the missing
    // parent directories of these binds itself.
    if let Some(socket) = agent_socket() {
        args.extend([OsString::from("--bind"), os(&socket), os(&socket)]);
    }

    // The project is the only writable directory of the sandbox.
    args.extend([OsString::from("--bind"), os(project), os(project)]);

    // A minimal `/dev` and a `/proc` that only knows the sandbox's
    // own pid namespace.
    args.extend([
        OsString::from("--dev"),
        os(Path::new("/dev")),
        OsString::from("--proc"),
        os(Path::new("/proc")),
    ]);

    // The GPU's device nodes on top of the minimal `/dev`; without
    // them the renderer cannot see the hardware and falls back to
    // the CPU.
    args.extend(gpu_binds(Path::new("/dev")));

    args.extend([
        OsString::from("--hostname"),
        OsString::from(HOSTNAME),
        OsString::from("--chdir"),
        os(project),
        // Die with the launcher and detach from its terminal.
        OsString::from("--die-with-parent"),
        OsString::from("--new-session"),
    ]);

    // A bare environment: no exported token reaches the model's
    // shell.
    args.extend([
        OsString::from("--clearenv"),
        OsString::from("--setenv"),
        OsString::from(MARKER),
        OsString::from("1"),
    ]);

    if let Some(value) = env::var_os("PATH") {
        args.extend([OsString::from("--setenv"), OsString::from("PATH"), value]);
    }

    if let Some(home) = home {
        args.extend([OsString::from("--setenv"), OsString::from("HOME"), os(home)]);
    }

    if let Some(value) = env::var_os("USER") {
        args.extend([OsString::from("--setenv"), OsString::from("USER"), value]);
    }

    // The ssh-agent socket the bind above put in place; `ssh` looks
    // for it here.
    if let Some(value) = env::var_os("SSH_AUTH_SOCK") {
        args.extend([
            OsString::from("--setenv"),
            OsString::from("SSH_AUTH_SOCK"),
            value,
        ]);
    }

    // The display variables the GUI needs to reach the compositor;
    // the socket they point at lives under `XDG_RUNTIME_DIR`, which
    // stays visible through the read-only root.
    for name in [
        "WAYLAND_DISPLAY",
        "DISPLAY",
        "XAUTHORITY",
        "XDG_RUNTIME_DIR",
    ] {
        if let Some(value) = env::var_os(name) {
            args.extend([OsString::from("--setenv"), OsString::from(name), value]);
        }
    }

    // `--clearenv` strips the locale with everything else, and the
    // bare C locale mangles non-ASCII output — the user's accented
    // name in gpg and git, for one — so mirror the host's locale
    // configuration into the sandbox; the host root is the sandbox
    // root, so the locale data is present. Where the host set
    // nothing, fall back to `C.UTF-8`.
    let host_env: Vec<(OsString, OsString)> = env::vars_os().collect();
    for (name, value) in locale_vars(&host_env) {
        args.extend([OsString::from("--setenv"), name, value]);
    }

    args.push(OsString::from("--"));
    args.push(os(exe));
    args
}

/// The locale variables to forward into the sandbox, chosen from an
/// environment: `LC_ALL`, any `LC_*`, and `LANG`, when set to a
/// non-empty value. When the environment has none of them — a daemon
/// start, say — a single `LC_ALL=C.UTF-8`, so output stays UTF-8
/// instead of falling into the bare C locale.
fn locale_vars(vars: &[(OsString, OsString)]) -> Vec<(OsString, OsString)> {
    let picked: Vec<(OsString, OsString)> = vars
        .iter()
        .filter(|(name, _)| {
            let name = name.to_string_lossy();
            name == "LANG" || name.starts_with("LC_")
        })
        .filter(|(_, value)| !value.is_empty())
        .cloned()
        .collect();

    if picked.is_empty() {
        vec![(OsString::from("LC_ALL"), OsString::from("C.UTF-8"))]
    } else {
        picked
    }
}

/// The ssh-agent socket the sandbox should see: the
/// `SSH_AUTH_SOCK` variable, when it names a live socket. The key
/// the agent holds stays on the host; only the socket crosses into
/// the sandbox.
fn agent_socket() -> Option<PathBuf> {
    env::var_os("SSH_AUTH_SOCK")
        .map(PathBuf::from)
        .filter(|socket| is_socket(socket))
}

/// The read-only binds ssh-format signing needs: the public key
/// file `user.signingkey` names and the signers list
/// `gpg.ssh.allowedSignersFile` names, when the global git
/// configuration in `home` sets them and the files exist. Both are
/// public data; the private key they describe stays on the host.
fn ssh_signing_binds(home: &Path) -> Vec<OsString> {
    let os = |path: &Path| path.as_os_str().to_os_string();

    let (signingkey, signers) = match git_config_values(&home.join(".gitconfig")) {
        Some(values) => values,
        None => return Vec::new(),
    };

    let mut args = Vec::new();
    for path in [signingkey, signers]
        .into_iter()
        .flatten()
        .map(PathBuf::from)
        .filter(|path| path.is_file())
    {
        args.extend([OsString::from("--ro-bind"), os(&path), os(&path)]);
    }
    args
}

/// The ssh-signing-relevant values of a git configuration file:
/// `user.signingkey` and `gpg.ssh.allowedSignersFile`, or `None`
/// when the file cannot be read. The signers value may be spelled
/// either as a dotted key under `[gpg]` or as a bare key under
/// `[gpg "ssh"]`; both are accepted. A value that does not name a
/// file — a fingerprint, the openpgp form of `user.signingkey` —
/// is left to the caller's filter.
fn git_config_values(path: &Path) -> Option<(Option<String>, Option<String>)> {
    let text = std::fs::read_to_string(path).ok()?;

    let mut section = String::new();
    let mut subsection = String::new();
    let mut signingkey: Option<String> = None;
    let mut signers: Option<String> = None;

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') || line.starts_with(';') {
            continue;
        }

        if let Some(header) = line
            .strip_prefix('[')
            .and_then(|rest| rest.strip_suffix(']'))
        {
            // `[section]` or `[section "subsection"]`
            let mut parts = header.trim().splitn(2, char::is_whitespace);
            section = parts.next().unwrap_or_default().to_ascii_lowercase();
            subsection = parts
                .next()
                .unwrap_or_default()
                .trim_matches('"')
                .to_ascii_lowercase();
            continue;
        }

        let (key, value) = match line.split_once('=') {
            Some((key, value)) => (key.trim().to_ascii_lowercase(), value.trim()),
            None => continue,
        };

        let full = if subsection.is_empty() {
            format!("{section}.{key}")
        } else {
            format!("{section}.{subsection}.{key}")
        };

        let value = value
            .strip_prefix('"')
            .and_then(|rest| rest.strip_suffix('"'))
            .unwrap_or(value);

        match full.as_str() {
            "user.signingkey" => signingkey = Some(value.to_owned()),
            "gpg.ssh.allowedsignersfile" => signers = Some(value.to_owned()),
            _ => {}
        }
    }

    Some((signingkey, signers))
}

/// Whether `path` names a socket, following any symlink the way an
/// agent socket may be one.
#[cfg(unix)]
fn is_socket(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    const S_IFSOCK: u32 = 0o14_0000;

    std::fs::metadata(path)
        .map(|meta| meta.mode() & 0o17_0000 == S_IFSOCK)
        .unwrap_or(false)
}

#[cfg(not(unix))]
fn is_socket(_path: &Path) -> bool {
    false
}

/// The X authorization file the X client libraries look up: the
/// `XAUTHORITY` variable, or `$HOME/.Xauthority` when it is unset.
fn xauthority(home: Option<&Path>) -> Option<PathBuf> {
    env::var_os("XAUTHORITY")
        .map(PathBuf::from)
        .or_else(|| home.map(|home| home.join(".Xauthority")))
}

/// The binds that keep the GPU visible in the sandbox, where the
/// minimal `/dev` of [`bwrap_args`] would otherwise hide it and
/// force the renderer onto the CPU.
///
/// The DRM nodes cover every GPU; on NVIDIA the driver also uses
/// its own nodes under `/dev`, so those are bound along with them.
///
/// These are `--dev-bind` mounts, not plain binds: the kernel
/// refuses to open a host device node from a user namespace unless
/// the mount itself allows device access, which is what
/// `--dev-bind` arranges.
///
/// The nodes keep their host ownership and mode, so access inside
/// the sandbox matches the launching user's access on the bare
/// host.
fn gpu_binds(dev: &Path) -> Vec<OsString> {
    let os = |path: &Path| path.as_os_str().to_os_string();

    let mut args = Vec::new();

    let drm = dev.join("dri");
    if drm.is_dir() {
        args.extend([OsString::from("--dev-bind"), os(&drm), os(&drm)]);
    }

    if let Ok(entries) = dev.read_dir() {
        for entry in entries.flatten() {
            if entry.file_name().to_string_lossy().starts_with("nvidia") {
                let path = entry.path();
                args.extend([OsString::from("--dev-bind"), os(&path), os(&path)]);
            }
        }
    }

    args
}

#[cfg(test)]
mod tests {
    use super::*;

    fn project() -> PathBuf {
        PathBuf::from("/home/user/code/pick")
    }

    fn home() -> PathBuf {
        PathBuf::from("/home/user")
    }

    fn exe() -> PathBuf {
        PathBuf::from("/usr/local/bin/pick")
    }

    /// Renders the argument list as strings for assertions.
    fn rendered(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn the_root_is_read_only_and_the_project_writable() {
        // A temp home without a `projects` directory: the exact
        // writability assertion must not depend on the machine it
        // runs on.
        let root = std::env::temp_dir()
            .join(format!("pick-sandbox-root-{}", std::process::id()))
            .join("home");
        std::fs::create_dir_all(&root).unwrap();

        let args = rendered(&bwrap_args(&project(), Some(&root), &exe()));

        assert!(
            args.windows(3)
                .any(|window| window == ["--ro-bind", "/", "/"])
        );

        // The GPU nodes also bind under `/dev`; they are not
        // writable places, so keep them out of this assertion. The
        // ssh-agent socket is a deliberate writable bind when
        // present (its own test covers it); exclude it here too.
        let socket = agent_socket().map(|socket| socket.to_string_lossy().into_owned());
        let writable = args
            .windows(3)
            .filter(|window| {
                window[0] == "--bind"
                    && !window[1].starts_with("/dev")
                    && socket.as_deref() != Some(window[1].as_str())
            })
            .map(|window| window.to_vec())
            .collect::<Vec<_>>();

        assert_eq!(
            writable,
            [vec![
                "--bind",
                "/home/user/code/pick",
                "/home/user/code/pick"
            ]]
        );

        // Scratch space is private to the sandbox.
        assert!(args.windows(2).any(|window| window == ["--tmpfs", "/tmp"]));
    }

    #[test]
    fn the_home_is_hidden_by_a_tmpfs() {
        let args = rendered(&bwrap_args(&project(), Some(&home()), &exe()));

        assert!(
            args.windows(2)
                .any(|window| window == ["--tmpfs", "/home/user"])
        );
    }

    #[test]
    fn the_toolchain_is_remounted_when_present() {
        let root = std::env::temp_dir()
            .join(format!("pick-sandbox-test-{}", std::process::id()))
            .join("home");
        std::fs::create_dir_all(root.join(".cargo")).unwrap();

        let args = rendered(&bwrap_args(&project(), Some(&root), &exe()));

        let cargo_home = root.join(".cargo").to_string_lossy().into_owned();
        assert!(
            args.windows(3)
                .any(|window| { window[0] == "--bind" && window[1] == cargo_home })
        );

        // No `~/.rustup` was created, so nothing is remounted for it.
        assert!(!args.iter().any(|arg| arg.ends_with(".rustup")));
    }

    #[test]
    fn the_projects_directory_is_bound_writable_when_present() {
        let root = std::env::temp_dir()
            .join(format!("pick-sandbox-projects-{}", std::process::id()))
            .join("home");
        std::fs::create_dir_all(root.join("projects")).unwrap();

        let args = rendered(&bwrap_args(&project(), Some(&root), &exe()));

        let projects = root.join("projects").to_string_lossy().into_owned();
        assert!(
            args.windows(3)
                .any(|window| { window == ["--bind", projects.as_str(), projects.as_str()] })
        );
    }

    #[test]
    fn the_projects_directory_is_not_bound_when_absent() {
        let root = std::env::temp_dir()
            .join(format!("pick-sandbox-projects-none-{}", std::process::id()))
            .join("home");
        std::fs::create_dir_all(&root).unwrap();

        let args = rendered(&bwrap_args(&project(), Some(&root), &exe()));

        assert!(!args.iter().any(|arg| arg.ends_with("/projects")));
    }

    #[test]
    fn a_live_sandbox_mounts_the_projects_directory_writable() {
        // Skipped where `bwrap` is unavailable, like the display
        // tests, and where the nested user namespace it needs
        // cannot be created, so the probe runs first.
        let Some(bwrap) = find_on_path(BINARY, env::var_os("PATH").as_deref()) else {
            return;
        };

        let probe = std::process::Command::new(&bwrap)
            .args([
                "--unshare-user",
                "--unshare-pid",
                "--ro-bind",
                "/",
                "/",
                "--dev",
                "/dev",
                "--proc",
                "/proc",
                "--",
                "true",
            ])
            .status();
        let Ok(status) = probe else {
            return;
        };
        if !status.success() {
            return;
        }

        let root = std::env::temp_dir()
            .join(format!("pick-sandbox-live-{}", std::process::id()))
            .join("home");
        let sibling = root.join("projects").join("sibling");
        let project = root.join("projects").join("pick");
        std::fs::create_dir_all(&sibling).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        std::fs::write(sibling.join("note.txt"), "reference\n").unwrap();

        let mut args = bwrap_args(&project, Some(&root), &exe());
        args.truncate(args.len() - 2); // Drop the `--` and the executable.
        let probe = format!(
            "test -f {s} && echo SIBLING_READABLE; \
             touch {p}/scratch 2>/dev/null && echo PROJECT_WRITABLE; \
             touch {sib}/scratch 2>/dev/null && echo SIBLING_WRITABLE || echo SIBLING_READONLY",
            s = sibling.join("note.txt").display(),
            p = project.display(),
            sib = sibling.display(),
        );
        args.extend([
            OsString::from("--"),
            OsString::from("sh"),
            OsString::from("-c"),
            OsString::from(&probe),
        ]);

        let output = match std::process::Command::new(&bwrap).args(&args).output() {
            // A spawn failure is an environment problem, not a
            // policy one; skip.
            Err(_) => return,
            Ok(output) => output,
        };
        let stdout = String::from_utf8_lossy(&output.stdout);
        if !output.status.success() {
            // Same: a nested sandbox that cannot start — a device
            // or namespace limit — is an environment problem.
            return;
        }

        assert!(stdout.contains("SIBLING_READABLE"), "{stdout}");
        assert!(stdout.contains("PROJECT_WRITABLE"), "{stdout}");
        assert!(stdout.contains("SIBLING_WRITABLE"), "{stdout}");
        assert!(!stdout.contains("SIBLING_READONLY"), "{stdout}");
    }

    #[test]
    fn an_executable_under_the_home_stays_reachable() {
        let args = rendered(&bwrap_args(
            &project(),
            Some(&home()),
            &home().join(".local/bin/pick"),
        ));

        assert!(
            args.windows(3)
                .any(|window| { window[0] == "--ro-bind" && window[1] == "/home/user/.local/bin" })
        );
    }

    #[test]
    fn an_executable_under_the_project_needs_no_extra_bind() {
        let args = rendered(&bwrap_args(
            &project(),
            Some(&home()),
            &project().join("target/debug/pick"),
        ));

        assert!(!args.windows(3).any(|window| {
            window[0] == "--ro-bind" && window[1] == "/home/user/code/pick/target/debug"
        }));
    }

    #[test]
    fn the_reexecution_ends_with_the_executable() {
        let args = rendered(&bwrap_args(&project(), Some(&home()), &exe()));

        let (penultimate, last) = (&args[args.len() - 2], &args[args.len() - 1]);
        assert_eq!(penultimate, "--");
        assert_eq!(last, "/usr/local/bin/pick");
    }

    #[test]
    fn the_environment_is_cleared_down_to_the_basics() {
        let args = rendered(&bwrap_args(&project(), Some(&home()), &exe()));

        assert!(args.contains(&"--clearenv".to_owned()));

        for name in ["PICK_SANDBOXED", "PATH", "HOME"] {
            assert!(
                args.windows(3)
                    .any(|window| window[0] == "--setenv" && window[1] == name),
                "missing {name}"
            );
        }
    }

    #[test]
    fn display_variables_are_forwarded_when_set() {
        // The test environment may run without a display, in which
        // case there is nothing to forward.
        if env::var_os("WAYLAND_DISPLAY").is_none() {
            return;
        }

        let args = rendered(&bwrap_args(&project(), Some(&home()), &exe()));

        assert!(
            args.windows(3)
                .any(|window| { window[0] == "--setenv" && window[1] == "WAYLAND_DISPLAY" })
        );
    }

    #[test]
    fn finds_the_binary_on_the_path() {
        let dir =
            std::env::temp_dir().join(format!("pick-sandbox-path-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        let binary = dir.join("bwrap");
        std::fs::write(&binary, "#!/bin/sh\nexit 0\n").unwrap();

        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o755)).unwrap();
        }

        assert_eq!(find_on_path("bwrap", Some(dir.as_os_str())), Some(binary));
        assert_eq!(find_on_path("no-such-binary", Some(dir.as_os_str())), None);
        assert_eq!(find_on_path("bwrap", None), None);
    }

    #[test]
    fn the_bare_reasons_read_like_notices() {
        assert_eq!(
            Reason::Unsupported.to_string(),
            "bubblewrap is only available on Linux"
        );
        assert_eq!(
            Reason::Missing.to_string(),
            "no `bwrap` binary on the PATH; install bubblewrap"
        );
        assert!(
            Reason::Probe("unshare failed: EPERM".to_owned())
                .to_string()
                .contains("EPERM")
        );
    }

    #[test]
    fn the_gpu_devices_are_bound_when_present() {
        let dev =
            std::env::temp_dir().join(format!("pick-sandbox-gpu-test-{}", std::process::id()));
        std::fs::create_dir_all(dev.join("dri")).unwrap();
        std::fs::write(dev.join("nvidiactl"), "").unwrap();
        std::fs::write(dev.join("nvidia0"), "").unwrap();
        std::fs::write(dev.join("zero"), "").unwrap();

        // `--dev-bind`, not `--bind`: the kernel refuses to open a
        // host device node from a user namespace unless the mount
        // allows device access.
        let binds: Vec<Vec<String>> = rendered(&gpu_binds(&dev))
            .windows(3)
            .filter(|window| window[0] == "--dev-bind")
            .map(|window| window.to_vec())
            .collect();

        let drm = dev.join("dri").to_string_lossy().into_owned();
        assert!(
            binds.contains(&vec![String::from("--dev-bind"), drm.clone(), drm]),
            "missing /dev/dri"
        );

        for name in ["nvidiactl", "nvidia0"] {
            let node = dev.join(name).to_string_lossy().into_owned();
            assert!(
                binds.contains(&vec![String::from("--dev-bind"), node.clone(), node]),
                "missing {name}"
            );
        }

        // Unrelated nodes stay out of the sandbox.
        assert!(!binds.iter().any(|bind| bind[1].ends_with("zero")));
    }

    #[test]
    fn an_empty_dev_needs_no_gpu_binds() {
        let dev = std::env::temp_dir().join(format!(
            "pick-sandbox-gpu-empty-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dev).unwrap();

        assert!(gpu_binds(&dev).is_empty());
    }

    #[test]
    fn the_x11_socket_directory_is_bound_when_present() {
        // The test environment may run without an X server, in
        // which case there is nothing to bind.
        if !Path::new("/tmp/.X11-unix").is_dir() {
            return;
        }

        let args = rendered(&bwrap_args(&project(), Some(&home()), &exe()));

        assert!(
            args.windows(3)
                .any(|window| window == ["--ro-bind", "/tmp/.X11-unix", "/tmp/.X11-unix"])
        );
    }

    #[test]
    fn the_x_authority_file_is_bound_when_present() {
        let args = rendered(&bwrap_args(&project(), Some(&home()), &exe()));

        // The file may not exist, and `XAUTHORITY` may point at a
        // missing file, in which case nothing is bound.
        let authority = xauthority(Some(&home())).filter(|path| path.is_file());

        match authority {
            Some(authority) => {
                assert!(args.windows(3).any(|window| {
                    window[0] == "--ro-bind" && window[1] == authority.to_string_lossy()
                }));
            }
            None => assert!(!args.iter().any(|arg| arg == ".Xauthority")),
        }
    }

    #[test]
    fn the_known_host_keys_are_bound_when_present() {
        let root = std::env::temp_dir()
            .join(format!("pick-sandbox-ssh-test-{}", std::process::id()))
            .join("home");
        std::fs::create_dir_all(root.join(".ssh")).unwrap();
        std::fs::write(
            root.join(".ssh").join("known_hosts"),
            "github.com ssh-ed25519 AAAA\n",
        )
        .unwrap();

        let args = rendered(&bwrap_args(&project(), Some(&root), &exe()));

        let known_hosts = root
            .join(".ssh")
            .join("known_hosts")
            .to_string_lossy()
            .into_owned();

        assert!(args.windows(3).any(|window| {
            window[0] == "--ro-bind" && window[1] == known_hosts && window[2] == known_hosts
        }));
    }

    #[test]
    fn the_known_host_keys_are_not_bound_when_absent() {
        let root = std::env::temp_dir()
            .join(format!(
                "pick-sandbox-ssh-empty-test-{}",
                std::process::id()
            ))
            .join("home");
        std::fs::create_dir_all(&root).unwrap();

        let args = rendered(&bwrap_args(&project(), Some(&root), &exe()));

        // No `known_hosts` in this home, so nothing is bound for it.
        assert!(!args.iter().any(|arg| arg.contains("known_hosts")));
    }

    #[test]
    fn the_agent_socket_is_bound_when_set() {
        // The test environment may run without an ssh agent, in
        // which case there is nothing to bind.
        let socket = match agent_socket() {
            Some(socket) => socket,
            None => return,
        };

        let args = rendered(&bwrap_args(&project(), Some(&home()), &exe()));

        let sock = socket.to_string_lossy().into_owned();
        assert!(
            args.windows(3)
                .any(|window| { window[0] == "--bind" && window[1] == sock })
        );

        assert!(
            args.windows(3)
                .any(|window| window[0] == "--setenv" && window[1] == "SSH_AUTH_SOCK")
        );
    }

    /// A temp home holding the two files ssh signing binds, for
    /// configuring a `.gitconfig` against; returns the home and the
    /// file paths.
    fn ssh_signing_home(root: &Path) -> (PathBuf, PathBuf, PathBuf) {
        let ssh = root.join(".ssh");
        std::fs::create_dir_all(&ssh).unwrap();
        let pub_key = ssh.join("id_ed25519.pub");
        std::fs::write(&pub_key, "ssh-ed25519 AAAA test\n").unwrap();
        let signers = ssh.join("allowed_signers");
        std::fs::write(&signers, "* ssh-ed25519 AAAA test\n").unwrap();
        (root.to_path_buf(), pub_key, signers)
    }

    #[test]
    fn the_ssh_signing_files_are_bound_when_configured() {
        let root = std::env::temp_dir()
            .join(format!("pick-sandbox-ssh-sign-{}", std::process::id()))
            .join("home");
        let (home, pub_key, signers) = ssh_signing_home(&root);
        std::fs::write(
            home.join(".gitconfig"),
            format!(
                "[user]\n\temail = x@y.z\n\tsigningkey = {pub}\n[gpg]\n\tformat = ssh\n\tssh.allowedSignersFile = {sig}\n",
                pub = pub_key.display(),
                sig = signers.display(),
            ),
        )
        .unwrap();

        let args = rendered(&bwrap_args(&project(), Some(&home), &exe()));

        for file in [&pub_key, &signers] {
            let path = file.to_string_lossy().into_owned();
            assert!(
                args.windows(3).any(|window| {
                    window[0] == "--ro-bind" && window[1] == path && window[2] == path
                }),
                "missing {path}"
            );
        }
    }

    #[test]
    fn the_signers_file_is_found_in_subsection_form() {
        let root = std::env::temp_dir()
            .join(format!("pick-sandbox-ssh-sign-sub-{}", std::process::id()))
            .join("home");
        let (home, pub_key, signers) = ssh_signing_home(&root);
        std::fs::write(
            home.join(".gitconfig"),
            format!(
                "[user]\n\tsigningkey = {pub}\n[gpg \"ssh\"]\n\tallowedSignersFile = {sig}\n",
                pub = pub_key.display(),
                sig = signers.display(),
            ),
        )
        .unwrap();

        let args = rendered(&bwrap_args(&project(), Some(&home), &exe()));

        for file in [&pub_key, &signers] {
            let path = file.to_string_lossy().into_owned();
            assert!(
                args.windows(3).any(|window| {
                    window[0] == "--ro-bind" && window[1] == path && window[2] == path
                }),
                "missing {path}"
            );
        }
    }

    #[test]
    fn a_fingerprint_signingkey_binds_nothing() {
        // In openpgp form `user.signingkey` is a fingerprint, not a
        // file, so no ssh-signing bind may appear for it.
        let root = std::env::temp_dir()
            .join(format!("pick-sandbox-ssh-sign-fpr-{}", std::process::id()))
            .join("home");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::write(
            root.join(".gitconfig"),
            "[user]\n\tsigningkey = 1B8B3BF178EB4639B145E8447CC46565708259A7\n[gpg]\n\tformat = ssh\n\tssh.allowedSignersFile = /nonexistent/signers\n",
        )
        .unwrap();

        let args = rendered(&bwrap_args(&project(), Some(&root), &exe()));

        assert!(!args.iter().any(|arg| arg.contains("1B8B3BF1")));
        assert!(!args.iter().any(|arg| arg.contains("signers")));
    }

    #[test]
    fn a_home_without_gitconfig_binds_no_ssh_signing() {
        let root = std::env::temp_dir()
            .join(format!("pick-sandbox-ssh-sign-none-{}", std::process::id()))
            .join("home");
        std::fs::create_dir_all(&root).unwrap();

        let args = rendered(&bwrap_args(&project(), Some(&root), &exe()));

        assert!(!args.iter().any(|arg| arg.contains("id_ed25519.pub")));
    }

    #[test]
    fn the_agent_socket_directory_is_blanked_when_set() {
        // The test environment may run without an ssh agent, in
        // which case there is nothing to blank.
        let socket = match agent_socket() {
            Some(socket) => socket,
            None => return,
        };

        let dir = match socket.parent() {
            Some(dir) if dir != Path::new("/") => dir,
            _ => return,
        };

        let args = rendered(&bwrap_args(&project(), Some(&home()), &exe()));

        let dir = dir.to_string_lossy().into_owned();
        let blank = args
            .windows(2)
            .position(|window| window[0] == "--tmpfs" && window[1] == dir)
            .unwrap_or_else(|| panic!("no tmpfs for {dir}"));

        // `--tmpfs` consumes a single destination argument. A
        // second copy of the path would be read as the command
        // bwrap execs, which once failed the start with "execvp
        // <dir>: Permission denied"; the directory must be named
        // exactly once.
        assert_ne!(
            args.get(blank + 2).map(String::as_str),
            Some(dir.as_str()),
            "the blank names {dir} twice"
        );

        // The blank comes first: it hides the other sockets, and
        // only the ssh one is bound back over it.
        let sock = socket.to_string_lossy().into_owned();
        let socket_bind = args
            .windows(3)
            .position(|window| window == ["--bind", sock.as_str(), sock.as_str()])
            .expect("the ssh-agent socket is bound back");
        assert!(
            blank < socket_bind,
            "the blank must precede the socket bind"
        );
    }

    #[test]
    fn locale_vars_forward_the_hosts_locale_when_set() {
        let vars = [
            (OsString::from("PATH"), OsString::from("/usr/bin")),
            (OsString::from("LANG"), OsString::from("en_US.UTF-8")),
            (OsString::from("LC_TIME"), OsString::from("de_DE.UTF-8")),
            (OsString::from("LC_ALL"), OsString::from("C")),
            (
                OsString::from("XDG_RUNTIME_DIR"),
                OsString::from("/run/user/1000"),
            ),
        ];

        assert_eq!(
            locale_vars(&vars),
            vec![
                (OsString::from("LANG"), OsString::from("en_US.UTF-8")),
                (OsString::from("LC_TIME"), OsString::from("de_DE.UTF-8")),
                (OsString::from("LC_ALL"), OsString::from("C")),
            ]
        );
    }

    #[test]
    fn locale_vars_fall_back_to_utf8_when_unset() {
        let vars = [
            (OsString::from("PATH"), OsString::from("/usr/bin")),
            (OsString::from("HOME"), OsString::from("/home/user")),
        ];

        assert_eq!(
            locale_vars(&vars),
            vec![(OsString::from("LC_ALL"), OsString::from("C.UTF-8"))]
        );
    }

    #[test]
    fn locale_vars_treat_an_empty_value_as_unset() {
        let vars = [(OsString::from("LANG"), OsString::from(""))];

        assert_eq!(
            locale_vars(&vars),
            vec![(OsString::from("LC_ALL"), OsString::from("C.UTF-8"))]
        );
    }

    #[test]
    fn the_locale_reaches_the_bwrap_arguments() {
        let host_env: Vec<(OsString, OsString)> = env::vars_os().collect();
        let expected = locale_vars(&host_env);

        let args = rendered(&bwrap_args(&project(), Some(&home()), &exe()));

        for (name, value) in &expected {
            let name = name.to_string_lossy().into_owned();
            let value = value.to_string_lossy().into_owned();
            assert!(
                args.windows(3)
                    .any(|window| window == ["--setenv", name.as_str(), value.as_str()]),
                "missing --setenv {name}={value}"
            );
        }
    }
}
