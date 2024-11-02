use std::path::Path;
use cargo_metadata::MetadataCommand;

/// Returns the value of the `MAKEPAD_PACKAGE_DIR` environment variable
/// that must be set for the given package format.
///
/// * For macOS app bundles, this should be set to the current directory `.`
///   * This only works because we enable the Makepad `apple_bundle` cfg option,
///     which tells Makepad to invoke Apple's `NSBundle` API to retrieve the resource path at runtime.
///     This resource path points to the bundle's `Contents/Resources/` directory.
/// * For AppImage packages, this should be set to the /usr/lib/<binary> directory. 
///   Since AppImages execute with a simulated working directory of `usr/`,
///   we just need a relative path that goes there, i.e.,  "lib/robrix`.
///   * Note that this must be a relative path, not an absolute path.
/// * For Debian `.deb` packages, this should be set to `/usr/lib/<main-binary-name>`.
///   * This is the directory in which `dpkg` copies app resource files to
///     when a user installs the `.deb` package.
/// * For Windows NSIS packages, this should be set to `.` (the current dir).
///  * This is because the NSIS installer script copies the resources to the same directory
///    as the installed binaries.
pub(crate) fn makepad_package_dir_value(package_format: &str, main_binary_name: &str) -> String {
    match package_format {
        "app" | "dmg" => format!("."),
        "appimage" => format!("lib/{}", main_binary_name),
        "deb" | "pacman" => format!("/usr/lib/{}", main_binary_name),
        "nsis" => format!("."),
        _other => panic!("Unsupported package format: {}", _other),
    }
}


/// Recursively copies the Makepad-specific resource files.
///
/// This uses `cargo-metadata` to determine the location of the `makepad-widgets` crate,
/// and then copies the `resources` directory from that crate to a makepad-specific subdirectory
/// of the given `dist_resources_dir` path, which is currently `./dist/resources/makepad_widgets/`.
pub(crate) fn copy_makepad_resources<P>(dist_resources_dir: P) -> std::io::Result<()>
where
    P: AsRef<Path>
{
    let makepad_widgets_resources_dest = dist_resources_dir.as_ref().join("makepad_widgets").join("resources");
    let makepad_widgets_resources_src = {
        let cargo_metadata = MetadataCommand::new()
            .exec()
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e))?;

        let makepad_widgets_package = cargo_metadata
            .packages
            .iter()
            .find(|package| package.name == "makepad-widgets")
            .ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "makepad-widgets package not found"),
            )?;

        makepad_widgets_package.manifest_path
            .parent()
            .ok_or_else(|| std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "makepad-widgets package manifest path not found"),
            )?
            .join("resources")
    };

    println!("Copying makepad-widgets resources...\n  --> From: {}\n      to:   {}",
        makepad_widgets_resources_src.as_std_path().display(),
        makepad_widgets_resources_dest.display(),
    );
    super::copy_recursively(&makepad_widgets_resources_src, &makepad_widgets_resources_dest)?;
    println!("  --> Done!");
    Ok(())
}
