//! Integration driver for the disposable Fedora RPM fixture, never shipped in packages.
#[cfg(target_os = "linux")]
fn main() -> Result<(), Box<dyn std::error::Error>> {
    if !std::path::Path::new("/run/.containerenv").is_file() {
        return Err("This transaction test driver requires the disposable test container".into());
    }
    let operation = std::env::args().nth(1).unwrap_or_else(|| "preview".into());
    if !matches!(
        operation.as_str(),
        "check" | "preview" | "execute" | "cancel" | "query"
    ) {
        return Err("Expected check, preview, execute, cancel, or query".into());
    }
    let mut updates = platform::linux::updates::RpmUpdates::current()?;
    let cancellation = updates.cancellation();
    let executing = std::cell::Cell::new(false);
    let mut progress = |progress: platform::updates::UpdateProgress| {
        if progress.stage == platform::updates::UpdateStage::StartingTransaction {
            executing.set(true);
        }
        println!("PROGRESS {progress:?}");
        if operation == "cancel" && executing.get() && progress.cancellable {
            cancellation.request();
        }
    };
    if operation == "query" {
        println!("QUERY {:?}", updates.query(&mut progress)?);
        return Ok(());
    }
    let check = updates.check(&mut progress)?;
    println!("CHECK {check:?}");
    if operation == "check" || check.candidate.is_none() {
        return Ok(());
    }
    let preview = updates.preview(&mut progress)?;
    println!("PREVIEW {preview:?}");
    if matches!(operation.as_str(), "execute" | "cancel") {
        println!("RESULT {:?}", updates.execute(&mut progress)?);
    }
    Ok(())
}
#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("The PackageKit fixture requires Fedora Linux");
}
