//! This program should be invoked by `cargo-packager` during its before-packaging steps.
//!
//! This program must be run from the root of the project directory,
//! which is also where the cargo-packager command must be invoked from.
//!
//! This program runs in two modes, one for each kind of before-packaging step in cargo-packager.
//! It requires passing in three arguments:
//! 1. An operating mode, either `before-packaging` or `before-each-packager`.
//!    * Passing in `before-packaging` specifies that the `before-packaging-command` is being run
//!      by cargo-packager, which gets executed only *once* before cargo-packager generates any package bundles.
//!    * Passing in `before-each-package` specifies that the `before-each-package-command` is being run
//!      by cargo-packager, which gets executed multiple times: once for *each* package that
//!      cargo-packager is going to generate.
//!       * The environment variable `CARGO_PACKAGER_FORMAT` is set by cargo-packager to
//!         the declare which package format is about to be generated, which include the values
//!         given here: <https://docs.rs/cargo-packager/latest/cargo_packager/enum.PackageFormat.html>.
//!         * `app`, `dmg`: for macOS.
//!         * `deb`, `appimage`, `pacman`: for Linux.
//!         * `nsis`: for Windows; `nsis` generates an installer `setup.exe`.
//!         * `wix`: (UNSUPPORTED) for Windows; generates an `.msi` installer package.
//! 2. The `--binary-name` argument, which specifies the name of the main binary that cargo-packager
//!    is going to package. This is the name of the binary that is generated by your app's main crate.
//! 3. The `--path-to-binary` argument, which specifies the path to the main binary that cargo-packager
//!    is going to package. This is the path to the binary that is generated by cargo when compiling
//!    your app's main crate.
//! 
//! This program uses the `CARGO_PACKAGER_FORMAT` environment variable to determine
//! which specific build commands and configuration options should be used.
//!

#[cfg(feature = "makepad")]
pub mod makepad;

use core::panic;
use std::{ffi::OsStr, fs, path::{Path, PathBuf}, process::{Command, Stdio}};

const EMPTY_ARGS: std::iter::Empty<&str> = std::iter::empty::<&str>();
const EMPTY_ENVS: std::iter::Empty<(&str, &str)> = std::iter::empty::<(&str, &str)>();


fn main() -> std::io::Result<()> {
    let mut is_before_packaging = false;
    let mut is_before_each_package = false;
    let mut main_binary_name = None;
    let mut path_to_binary = None;
    let mut host_os_opt: Option<String> = None;

    let mut args = std::env::args().peekable();
    while let Some(arg) = args.next() {
        if arg.ends_with("before-packaging") || arg.ends_with("before_packaging") {
            is_before_packaging = true;
        }
        if arg.contains("before-each") || arg.contains("before_each") {
            is_before_each_package = true;
        }
        if arg == "--binary-name" {
            main_binary_name = Some(args.next().expect("Expected a binary name after '--binary-name'."));
        }
        if arg == "--path-to-binary" {
            let path = PathBuf::from(args.next().expect("Expected a path after '--path-to-binary'."));
            path_to_binary = Some(path);
        }
        if host_os_opt.is_none() && (arg.contains("host_os") || arg.contains("host-os")) {
            host_os_opt = arg
                .split("=")
                .last()
                .map(|s| s.to_string())
                .or_else(|| args.peek().map(|s| s.to_string()));
        }
    }

    let main_binary_name = main_binary_name.expect("Missing required argument '--binary-name'");
    let path_to_binary = path_to_binary.expect("Missing required argument '--path-to-binary'");
    let host_os = host_os_opt.as_deref().unwrap_or(std::env::consts::OS);

    match (is_before_packaging, is_before_each_package) {
        (true, false) => before_packaging(host_os, &main_binary_name),
        (false, true) => before_each_package(host_os, &main_binary_name, &path_to_binary),
        (true, true) => panic!("Cannot run both 'before-packaging' and 'before-each-package' commands at the same time."),
        (false, false) => panic!("Please specify either the 'before-packaging' or 'before-each-package' command."),
    }
}

/// This function is run only *once* before cargo-packager generates any package bundles.
///
/// ## Functionality
/// 1. Creates a directory for the resources to be packaged, which is currently `./dist/resources/`.
/// 2. If the `makepad` feature is enabled, handles locating and copying in makepad-specific resource files.
/// 3. Recursively copies the app-specific `./resources` directory to `./dist/resources/<main-binary-name>/`.
fn before_packaging(_host_os: &str, main_binary_name: &str) -> std::io::Result<()> {
    let cwd = std::env::current_dir()?;
    let dist_resources_dir = cwd.join("dist").join("resources");
    fs::create_dir_all(&dist_resources_dir)?;
    
    #[cfg(feature = "makepad")] {
        makepad::copy_makepad_resources(&dist_resources_dir)?;
    }
    
    let app_resources_dest = dist_resources_dir.join(main_binary_name).join("resources");
    let app_resources_src = cwd.join("resources");
    println!("Copying app-specific resources...\n  --> From {}\n      to:   {}", app_resources_src.display(), app_resources_dest.display());
    copy_recursively(&app_resources_src, &app_resources_dest)?;
    println!("  --> Done!");
    Ok(())
}


/// Copy files from source to destination recursively.
fn copy_recursively(source: impl AsRef<Path>, destination: impl AsRef<Path>) -> std::io::Result<()> {
    fs::create_dir_all(&destination)?;
    for entry in fs::read_dir(source)? {
        let entry = entry?;
        let filetype = entry.file_type()?;
        if filetype.is_dir() {
            copy_recursively(entry.path(), destination.as_ref().join(entry.file_name()))?;
        } else {
            fs::copy(entry.path(), destination.as_ref().join(entry.file_name()))?;
        }
    }
    Ok(())
}


/// The function that is run by cargo-packager's `before-each-package-command`.
///
/// It's just a simple wrapper that invokes the function for each specific package format.
fn before_each_package<P: AsRef<Path>>(
    host_os: &str,
    main_binary_name: &str,
    path_to_binary: P,
) -> std::io::Result<()> {
    // The `CARGO_PACKAGER_FORMAT` environment variable is required.
    let format = std::env::var("CARGO_PACKAGER_FORMAT")
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

    let package_format = format.as_str();
    println!("Running before-each-package-command for {package_format:?}");
    match package_format         {
        "app" | "dmg" => before_each_package_macos(   package_format, host_os, &main_binary_name, &path_to_binary),
        "deb"         => before_each_package_deb(     package_format, host_os, &main_binary_name, &path_to_binary),
        "appimage"    => before_each_package_appimage(package_format, host_os, &main_binary_name, &path_to_binary),
        "pacman"      => before_each_package_pacman(  package_format, host_os, &main_binary_name, &path_to_binary),
        "nsis"        => before_each_package_windows( package_format, host_os, &main_binary_name, &path_to_binary),
        _other => return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Unknown/unsupported package format {_other:?}"),
        )),
    }
}


/// Runs the macOS-specific build commands for "app" and "dmg" package formats.
///
/// This function effectively runs the following shell commands:
/// ```sh
///    MAKEPAD_PACKAGE_DIR=../Resources  cargo build --workspace --release \
///    && install_name_tool -add_rpath "@executable_path/../Frameworks" ./target/release/_moly_app;
/// ```
fn before_each_package_macos<P: AsRef<Path>>(
    package_format: &str,
    host_os: &str,
    main_binary_name: &str,
    path_to_binary: P,
) -> std::io::Result<()> {
    assert!(host_os == "macos", "'app' and 'dmg' packages can only be created on macOS.");

    #[cfg(feature = "makepad")]
    let extra_envs = [("MAKEPAD", "apple_bundle")];
    #[cfg(not(feature = "makepad"))]
    let extra_envs = EMPTY_ENVS;

    cargo_build(
        package_format,
        host_os,
        main_binary_name,
        EMPTY_ARGS,
        extra_envs,
    )?;

    // Use `install_name_tool` to add the `@executable_path` rpath to the binary.
    let install_name_tool_cmd = Command::new("install_name_tool")
        .arg("-add_rpath")
        .arg("@executable_path/../Frameworks")
        .arg(path_to_binary.as_ref())
        .spawn()?;

    let output = install_name_tool_cmd.wait_with_output()?;
    if !output.status.success() {
        eprintln!("Failed to run install_name_tool command: {}
            ------------------------- stderr: -------------------------
            {:?}",
            output.status,
            String::from_utf8_lossy(&output.stderr),
        );
        return Err(std::io::Error::new(std::io::ErrorKind::Other, "Failed to run install_name_tool command for macOS"));
    }

    Ok(())
}

/// Runs the Linux-specific build commands for AppImage packages.
fn before_each_package_appimage<P: AsRef<Path>>(
    package_format: &str,
    host_os: &str,
    main_binary_name: &str,
    path_to_binary: P,
) -> std::io::Result<()> {
    assert!(host_os == "linux", "AppImage packages can only be created on Linux.");

    cargo_build(
        package_format,
        host_os,
        main_binary_name,
        EMPTY_ARGS,
        EMPTY_ENVS,
    )?;

    strip_unneeded_linux_binaries(host_os, path_to_binary)?;
    Ok(())
}


/// Runs the Linux-specific build commands for Debian `.deb` packages.
///
/// This function effectively runs the following shell commands:
/// ```sh
///    for path in $(ldd target/release/_moly_app | awk '{print $3}'); do \
///        basename "$/path" ; \
///    done \
///    | xargs dpkg -S 2> /dev/null | awk '{print $1}' | awk -F ':' '{print $1}' | sort | uniq > ./dist/depends_deb.txt; \
///    echo "curl" >> ./dist/depends_deb.txt; \
///    
fn before_each_package_deb<P: AsRef<Path>>(
    package_format: &str,
    host_os: &str,
    main_binary_name: &str,
    path_to_binary: P,
) -> std::io::Result<()> {
    assert!(host_os == "linux", "'deb' packages can only be created on Linux.");

    cargo_build(
        package_format,
        host_os,
        main_binary_name,
        EMPTY_ARGS,
        EMPTY_ENVS,
    )?;


    // Create Debian dependencies file by running `ldd` on the binary
    // and then running `dpkg -S` on each unique shared libraries outputted by `ldd`.
    let ldd_output = Command::new("ldd")
        .arg(path_to_binary.as_ref())
        .output()?;

    let ldd_output = if ldd_output.status.success() {
        String::from_utf8_lossy(&ldd_output.stdout)
    } else {
        eprintln!("Failed to run ldd command: {}
            ------------------------- stderr: -------------------------
            {:?}",
            ldd_output.status,
            String::from_utf8_lossy(&ldd_output.stderr),
        );
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to run ldd command on {host_os} for package format {package_format:?}")
        ));
    };

    let mut dpkgs = Vec::new();
    for line_raw in ldd_output.lines() {
        let line = line_raw.trim();
        let lib_name_opt = line.split_whitespace()
            .next()
            .and_then(|path| Path::new(path)
                .file_name()
                .and_then(|f| f.to_str().to_owned())
            );
        let Some(lib_name) = lib_name_opt else { continue };

        let dpkg_output = Command::new("dpkg")
            .arg("-S")
            .arg(lib_name)
            .stderr(Stdio::null())
            .output()?;
        let dpkg_output = if dpkg_output.status.success() {
            String::from_utf8_lossy(&dpkg_output.stdout)
        } else {
            // Skip shared libraries that dpkg doesn't know about, e.g., `linux-vdso.so*`
            continue;
        };

        let Some(package_name) = dpkg_output.split(':').next() else { continue };
        println!("Got dpkg dependency {package_name:?} from ldd output: {line:?}");
        dpkgs.push(package_name.to_string());
    }
    dpkgs.sort();
    dpkgs.dedup();
    println!("Sorted and de-duplicated dependencies: {:#?}", dpkgs);
    std::fs::write("./dist/depends_deb.txt", dpkgs.join("\n"))?;
    
    strip_unneeded_linux_binaries(host_os, path_to_binary)?;
    Ok(())
}


/// Runs the Linux-specific build commands for PacMan packages.
///
/// This is untested and may be incomplete, e.g., dependencies are not determined.
fn before_each_package_pacman<P: AsRef<Path>>(
    package_format: &str,
    host_os: &str,
    main_binary_name: &str,
    path_to_binary: P,
) -> std::io::Result<()> {
    assert!(host_os == "linux", "Pacman packages can only be created on Linux.");

    cargo_build(
        package_format,
        host_os,
        main_binary_name,
        EMPTY_ARGS,
        EMPTY_ENVS,
    )?;

    strip_unneeded_linux_binaries(host_os, path_to_binary)?;
    Ok(())
}
    
/// Runs the Windows-specific build commands for WiX (`.msi`) and NSIS (`.exe`) packages.
fn before_each_package_windows<P: AsRef<Path>>(
    package_format: &str,
    host_os: &str,
    main_binary_name: &str,
    _path_to_binary: P,
) -> std::io::Result<()> {
    assert!(host_os == "windows", "'.exe' and '.msi' packages can only be created on Windows.");

    cargo_build(
        package_format,
        host_os,
        main_binary_name,
        EMPTY_ARGS,
        EMPTY_ENVS,
    )?;

    Ok(())
}

fn cargo_build<I, A, E, K, V>(
    package_format: &str,
    _host_os: &str,
    _main_binary_name: &str,
    extra_args: I,
    extra_envs: E,
) -> std::io::Result<()>
where
    I: IntoIterator<Item = A>,
    A: AsRef<OsStr>,
    E: IntoIterator<Item = (K, V)>,
    K: AsRef<OsStr>,
    V: AsRef<OsStr>,
{
    let mut cargo_build_cmd = Command::new("cargo");
    cargo_build_cmd
        .arg("build")
        .arg("--workspace")
        .arg("--release")
        .args(extra_args)
        .envs(extra_envs);

    #[cfg(feature = "makepad")] {
        cargo_build_cmd.env(
            "MAKEPAD_PACKAGE_DIR",
            &makepad::makepad_package_dir_value(package_format, _main_binary_name),
        );
    }

    let output = cargo_build_cmd
        .spawn()?
        .wait_with_output()?;
    if !output.status.success() {
        eprintln!("Failed to run cargo build command: {}
            ------------------------- stderr: -------------------------
            {:?}",
            output.status,
            String::from_utf8_lossy(&output.stderr),
        );
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            format!("Failed to run cargo build command on {_host_os} for package format {package_format:?}")
        ));
    }

    Ok(())
}

/// Strips unneeded symbols from the Linux binary, which is required for Debian `.deb` packages
/// and recommended for all other Linux package formats.
fn strip_unneeded_linux_binaries<P: AsRef<Path>>(host_os: &str, path_to_binary: P) -> std::io::Result<()> {
    assert!(host_os == "linux", "'strip --strip-unneeded' can only be run on Linux.");
    let strip_cmd = Command::new("strip")
        .arg("--strip-unneeded")
        .arg("--remove-section=.comment")
        .arg("--remove-section=.note")
        .arg(path_to_binary.as_ref())
        .spawn()?;

    let output = strip_cmd.wait_with_output()?;
    if !output.status.success() {
        eprintln!("Failed to run strip command: {}
            ------------------------- stderr: -------------------------
            {:?}",
            output.status,
            String::from_utf8_lossy(&output.stderr),
        );
        return Err(std::io::Error::new(std::io::ErrorKind::Other, "Failed to run strip command for Linux"));
    }

    Ok(())
}
