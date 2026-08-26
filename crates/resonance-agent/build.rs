fn main() {
    println!("cargo:rerun-if-changed=../../assets/branding/resonance-signal.rc");
    println!("cargo:rerun-if-changed=../../assets/branding/resonance-signal-icon.ico");
    println!("cargo:rerun-if-changed=../../assets/branding/resonance-signal-tray.ico");

    #[cfg(windows)]
    embed_resource::compile_for(
        "../../assets/branding/resonance-signal.rc",
        ["resonance-agent"],
        embed_resource::NONE,
    )
    .manifest_required()
    .expect("failed to embed Resonance Signal Windows icon resources");
}
