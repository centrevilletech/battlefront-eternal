//! macOS: an update has to come from whoever signed the app it updates.
//!
//! A build reaches a player through the signed update channel, and every file of it is
//! checked against that channel. This is the second wall, for the day the channel's key or
//! the server that holds it is in other hands. macOS asks who signed a program only when a
//! browser downloaded it, never for files this program writes, so the updater asks itself:
//! before a downloaded version counts as installed, before a start hands off to it, and
//! before it takes the installed app's place, its bundle has to verify as a whole and
//! satisfy the requirement that the app's own signature states for later versions of itself
//! (its designated requirement). For a build signed with a Developer ID that reads "this
//! bundle identifier, a Developer ID certificate issued by Apple, this team". Nothing about
//! any team is written here: the installed app is the reference.
//!
//! An app with no certificate behind its signature (a developer's own build, signed ad hoc)
//! states no such requirement and takes whatever its channel offers, as the other systems
//! do.

use std::path::Path;
use std::process::Command;

const CODESIGN: &str = "/usr/bin/codesign";

/// Who has to have signed an update of an app.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Signer {
    /// No certificate signed the app: any build its channel offers is taken.
    Anyone,
    /// The app's designated requirement, in the text form `codesign` prints and reads.
    Requirement(String),
}

/// Reads from an app bundle's signature who has to sign its updates.
///
/// An error means the signature could not be read; the caller then takes no update rather
/// than one it cannot check.
pub fn signer_of(bundle: &Path) -> Result<Signer, String> {
    if !cfg!(target_os = "macos") {
        return Ok(Signer::Anyone);
    }
    let output = Command::new(CODESIGN)
        .args(["--display", "--requirements", "-"])
        .arg(bundle)
        .output()
        .map_err(|e| format!("cannot run codesign: {e}"))?;
    let errors = String::from_utf8_lossy(&output.stderr);
    if !output.status.success() {
        // Not even an ad hoc signature: a program somebody built and never signed.
        if errors.contains("not signed at all") {
            return Ok(Signer::Anyone);
        }
        return Err(format!(
            "cannot read the signature of {}: {}",
            bundle.display(),
            errors.trim()
        ));
    }
    let printed = format!("{}\n{errors}", String::from_utf8_lossy(&output.stdout));
    parse_designated(&printed).ok_or_else(|| {
        format!(
            "the signature of {} states no requirement",
            bundle.display()
        )
    })
}

/// The signer in what `codesign --display --requirements -` printed.
fn parse_designated(printed: &str) -> Option<Signer> {
    let text = printed.lines().find_map(|line| {
        line.split_once("designated => ")
            .map(|(_, requirement)| requirement.trim())
    })?;
    if text.is_empty() {
        return None;
    }
    // An ad hoc signature names nobody, only the one build it is on (its code directory's hash).
    if text.starts_with("cdhash ") {
        return Some(Signer::Anyone);
    }
    Some(Signer::Requirement(text.to_string()))
}

/// Checks an app bundle on disk: every file is as its signature seals it and, where `signer`
/// states a requirement, that signature satisfies it.
pub fn check_bundle(bundle: &Path, signer: &Signer) -> Result<(), String> {
    let mut command = Command::new(CODESIGN);
    command.args(["--verify", "--deep", "--strict"]);
    if let Signer::Requirement(text) = signer {
        // "=" marks requirement text (rather than a file that holds one).
        command.arg(format!("-R={text}"));
    }
    let output = command
        .arg(bundle)
        .output()
        .map_err(|e| format!("cannot run codesign: {e}"))?;
    if output.status.success() {
        return Ok(());
    }
    let said = String::from_utf8_lossy(&output.stderr);
    let said = said.trim();
    Err(if said.is_empty() {
        "codesign refused it".to_string()
    } else {
        said.replace('\n', "; ")
    })
}

/// Test bundles with real signatures (macOS only: `codesign` makes them).
#[cfg(all(test, target_os = "macos"))]
pub(crate) mod testing {
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::path::{Path, PathBuf};
    use std::process::Command;

    /// Writes `<parent>/Battlefront Eternal.app` as build/native/package-macos.py lays it out,
    /// holding `version`, and signs it ad hoc under `identifier`. Returns the bundle.
    pub(crate) fn signed_bundle(parent: &Path, version: &str, identifier: &str) -> PathBuf {
        let bundle = unsigned_bundle(parent, version, identifier);
        sign(&bundle);
        bundle
    }

    pub(crate) fn unsigned_bundle(parent: &Path, version: &str, identifier: &str) -> PathBuf {
        let bundle = parent.join("Battlefront Eternal.app");
        let macos = bundle.join("Contents/MacOS");
        fs::create_dir_all(&macos).unwrap();
        fs::create_dir_all(bundle.join("Contents/Resources")).unwrap();
        // A real program: a signature seals a bundle through its main executable.
        fs::copy("/usr/bin/true", macos.join("bfe-client")).unwrap();
        fs::set_permissions(macos.join("bfe-client"), fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink("../Resources", macos.join("resources")).unwrap();
        fs::write(
            bundle.join("Contents/Info.plist"),
            format!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<plist version=\"1.0\"><dict>\
                 <key>CFBundleExecutable</key><string>bfe-client</string>\
                 <key>CFBundleIdentifier</key><string>{identifier}</string>\
                 <key>CFBundlePackageType</key><string>APPL</string></dict></plist>\n"
            ),
        )
        .unwrap();
        fs::write(
            bundle.join("Contents/Resources/bfe-release.json"),
            format!(r#"{{"version":"{version}","channel":"development-preview"}}"#),
        )
        .unwrap();
        bundle
    }

    pub(crate) fn sign(bundle: &Path) {
        let output = Command::new(super::CODESIGN)
            .args(["--force", "--sign", "-"])
            .arg(bundle)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "codesign: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_who_has_to_sign_an_update_from_what_codesign_prints() {
        let developer = "Executable=/Applications/Battlefront Eternal.app/Contents/MacOS/bfe-client\n\
            designated => identifier \"com.battlefronteternal.client\" and anchor apple generic and \
            certificate 1[field.1.2.840.113635.100.6.2.6] /* exists */ and \
            certificate leaf[field.1.2.840.113635.100.6.1.13] /* exists */ and certificate leaf[subject.OU] = TEAM000001\n";
        let Some(Signer::Requirement(text)) = parse_designated(developer) else {
            panic!("a Developer ID signature states a requirement");
        };
        assert!(text
            .starts_with("identifier \"com.battlefronteternal.client\" and anchor apple generic"));
        assert!(text.ends_with("certificate leaf[subject.OU] = TEAM000001"));
        // Ad hoc: the requirement is the build itself, which no update can be.
        let ad_hoc =
            "Executable=/x\n# designated => cdhash H\"f2ac58ddda3a4e72b5f6014566fd5eb038378101\"\n";
        assert_eq!(parse_designated(ad_hoc), Some(Signer::Anyone));
        assert_eq!(parse_designated("Executable=/x\n"), None);
        assert_eq!(parse_designated("designated => \n"), None);
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn checks_a_real_signature_against_a_requirement() {
        let work = std::env::temp_dir().join(format!("bfe-signer-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&work);
        let bundle =
            testing::signed_bundle(&work, "0.1.0-preview.2", "com.battlefronteternal.test");
        assert_eq!(
            signer_of(&bundle),
            Ok(Signer::Anyone),
            "an ad hoc signature names nobody"
        );
        assert_eq!(check_bundle(&bundle, &Signer::Anyone), Ok(()));
        let same = Signer::Requirement("identifier \"com.battlefronteternal.test\"".to_string());
        assert_eq!(check_bundle(&bundle, &same), Ok(()));
        // A certificate requirement, as a signed app states it: an ad hoc build does not meet it.
        let developer = Signer::Requirement(
            "identifier \"com.battlefronteternal.test\" and certificate leaf = H\"55ebc6f862f05927eae0fb1120a34b03c0bd30da\""
                .to_string(),
        );
        assert!(check_bundle(&bundle, &developer)
            .unwrap_err()
            .contains("requirement"));
        // A file changed after signing: refused whoever is asked for.
        std::fs::write(bundle.join("Contents/Resources/bfe-release.json"), "{}").unwrap();
        assert!(check_bundle(&bundle, &Signer::Anyone).is_err());
        assert!(check_bundle(&bundle, &same).is_err());
        // A program with no signature at all takes any update too.
        let bare = testing::unsigned_bundle(
            &work.join("bare"),
            "0.1.0-preview.2",
            "com.battlefronteternal.test",
        );
        let _ = std::process::Command::new(CODESIGN)
            .arg("--remove-signature")
            .arg(bare.join("Contents/MacOS/bfe-client"))
            .output();
        assert_eq!(signer_of(&bare), Ok(Signer::Anyone));
        let _ = std::fs::remove_dir_all(&work);
    }
}
