// Icône et nom de l'application dans l'exe.
fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico")
            .set("FileDescription", "Tâches")
            .set("TachesCli", "ajouter")
            .set("ProductName", "Tâches")
            .set("InternalName", "taches")
            .set("OriginalFilename", "Taches.exe")
            .set("CompanyName", "Desarrh")
            .set("LegalCopyright", "MIT");
        if let Err(e) = res.compile() {
            println!("cargo:warning=icône non intégrée : {e}");
        }
    }
}
