//! Puts YtFast's icon and name into the Windows program file.

fn main() {
    let resource = "../../packaging/windows/ytfast.rc";
    println!("cargo::rerun-if-changed={resource}");
    println!("cargo::rerun-if-changed=../../packaging/icons/ytfast.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_resource::compile(resource, embed_resource::NONE)
            .manifest_optional()
            .expect("the Windows resources compile");
    }
}
