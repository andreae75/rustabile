//! Mette l'icona dentro il programma. `rustabile.ico` è ricavata da
//! `data/icons/io.github.andreae75.Rustabile.svg` (16–256 pixel).

fn main() {
    println!("cargo:rerun-if-changed=rustabile.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut risorse = winresource::WindowsResource::new();
    risorse.set_icon("rustabile.ico");
    // Il nome che Windows mostra in "Apri con" e in Gestione attività.
    risorse.set("FileDescription", "Rustabile");
    risorse.set("ProductName", "Rustabile");
    risorse
        .compile()
        .expect("impossibile aggiungere l'icona al programma");
}
