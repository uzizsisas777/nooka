use std::process::Command;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let target = std::env::var("TARGET").unwrap();
    let out_dir = std::env::var("OUT_DIR").unwrap();

    if target.contains("windows") {
        let windres = "C:\\Users\\ediss\\Downloads\\mingw64\\bin\\windres.exe";
        let rc_file = format!("{}\\resources.rc", manifest_dir);
        let output = format!("{}\\resources.o", out_dir);

        let status = Command::new(windres)
            .args(&[&rc_file, "-o", &output])
            .status()
            .expect("Failed to run windres");

        if !status.success() {
            panic!("windres failed");
        }

        println!("cargo:rustc-link-arg={}", output);
        // El resources.o de windres revierte el PE a CONSOLE en target GNU; forzar subsystem
        // windows explícito para que el setup nunca enseñe una consola.
        println!("cargo:rustc-link-arg=-Wl,--subsystem,windows");
        println!("cargo:rerun-if-changed=resources.rc");
        println!("cargo:rerun-if-changed=app.manifest");
        println!("cargo:rerun-if-changed=icon.ico");
    }
}
