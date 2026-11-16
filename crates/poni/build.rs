fn main() {
    println!("cargo:rerun-if-changed=res/poni.rc");
    println!("cargo:rerun-if-changed=res/poni.manifest");
    println!("cargo:rerun-if-changed=res/poni.ico");
    // Only Windows targets get the icon, manifest (Per-Monitor-V2 DPI) and version info.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let root = std::path::PathBuf::from(
            std::env::var("CARGO_MANIFEST_DIR").expect("Cargo sets CARGO_MANIFEST_DIR"),
        );
        let out = std::path::PathBuf::from(std::env::var("OUT_DIR").expect("Cargo sets OUT_DIR"));
        let resource_path = |path: &std::path::Path| path.to_string_lossy().replace('\\', "\\\\");
        let version = std::env::var("CARGO_PKG_VERSION").expect("Cargo sets CARGO_PKG_VERSION");
        let manifest = std::fs::read_to_string(root.join("res/poni.manifest"))
            .expect("Windows manifest template is needed")
            .replace("@VERSION@", &version);
        let manifest_file = out.join("poni.manifest");
        std::fs::write(&manifest_file, manifest).expect("Windows manifest could not be written");
        let components = ["MAJOR", "MINOR", "PATCH"].map(|part| {
            std::env::var(format!("CARGO_PKG_VERSION_{part}"))
                .expect("Cargo sets version components")
        });
        let numeric_version = format!("{},{},{},0", components[0], components[1], components[2]);
        // 资源版本跟随 Cargo，生成文件放在 OUT_DIR，避免手工修改生成结果。
        let resource = std::fs::read_to_string(root.join("res/poni.rc"))
            .expect("Windows resource template is needed")
            .replace("@ICON_PATH@", &resource_path(&root.join("res/poni.ico")))
            .replace("@MANIFEST_PATH@", &resource_path(&manifest_file))
            .replace("@VERSION_COMPONENTS@", &numeric_version)
            .replace("@VERSION@", &version);
        let resource_file = out.join("poni.rc");
        std::fs::write(&resource_file, resource)
            .expect("Windows resource file could not be written");
        // Panics with a clear message if rc.exe / x86_64-w64-mingw32-windres is missing.
        embed_resource::compile(&resource_file, embed_resource::NONE);
        // MSVC builds: resolve statically imported non-KnownDLLs (d2d1, dwrite,
        // dwmapi, winmm) from System32 only, never from the exe's folder.
        if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
            println!("cargo:rustc-link-arg-bins=/DEPENDENTLOADFLAG:0x800");
        }
    }
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("macos") {
        build_location();
        build_translation();
        build_attachment_preview();
    }
    build_info();
}

fn build_location() {
    use std::process::Command;
    println!("cargo:rerun-if-changed=src/mac/services/location.m");
    let out = std::env::var("OUT_DIR").expect("Cargo sets OUT_DIR");
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64",
        Ok("x86_64") => "x86_64",
        _ => panic!("unsupported Mac architecture"),
    };
    let object = format!("{out}/location.o");
    let library = format!("{out}/libponi_location.a");
    let status = Command::new("xcrun")
        .args([
            "clang",
            "-arch",
            arch,
            "-mmacosx-version-min=14.0",
            "-fobjc-arc",
            "-fblocks",
            "-c",
            "src/mac/services/location.m",
            "-o",
            &object,
        ])
        .status()
        .expect("xcrun clang is needed to build Mac location support");
    assert!(status.success(), "CoreLocation bridge did not compile");
    let status = Command::new("xcrun")
        .args(["ar", "crs", &library, &object])
        .status()
        .expect("xcrun ar is needed");
    assert!(status.success(), "CoreLocation bridge did not archive");
    println!("cargo:rustc-link-search=native={out}");
    println!("cargo:rustc-link-lib=static=poni_location");
    println!("cargo:rustc-link-lib=framework=CoreLocation");
}

fn build_attachment_preview() {
    use std::process::Command;
    println!("cargo:rerun-if-changed=src/mac/services/attachment_preview.m");
    let out = std::env::var("OUT_DIR").expect("Cargo sets OUT_DIR");
    let arch = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64",
        Ok("x86_64") => "x86_64",
        _ => panic!("unsupported Mac architecture"),
    };
    let object = format!("{out}/attachment_preview.o");
    let library = format!("{out}/libponi_attachment_preview.a");
    let status = Command::new("xcrun")
        .args([
            "clang",
            "-arch",
            arch,
            "-mmacosx-version-min=14.0",
            "-fobjc-arc",
            "-c",
            "src/mac/services/attachment_preview.m",
            "-o",
            &object,
        ])
        .status()
        .expect("xcrun clang is needed to build Mac attachment preview support");
    assert!(
        status.success(),
        "Attachment preview bridge did not compile"
    );
    let status = Command::new("xcrun")
        .args(["ar", "crs", &library, &object])
        .status()
        .expect("xcrun ar is needed");
    assert!(
        status.success(),
        "Attachment preview bridge did not archive"
    );
    println!("cargo:rustc-link-search=native={out}");
    println!("cargo:rustc-link-lib=static=poni_attachment_preview");
    println!("cargo:rustc-link-lib=framework=QuickLookUI");
}

fn build_translation() {
    use std::path::Path;
    use std::process::Command;

    println!("cargo:rerun-if-changed=src/mac/services/translation.swift");
    let out = std::env::var("OUT_DIR").expect("Cargo sets OUT_DIR");
    let target = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("aarch64") => "arm64-apple-macos14.0",
        Ok("x86_64") => "x86_64-apple-macos14.0",
        _ => panic!("unsupported Mac architecture"),
    };
    let object = format!("{out}/translation.o");
    let library = format!("{out}/libponi_translation.a");
    let status = Command::new("xcrun")
        .args([
            "swiftc",
            "-swift-version",
            "6",
            "-parse-as-library",
            "-emit-object",
            "-O",
            "-target",
            target,
            "-Xfrontend",
            "-disable-autolink-framework",
            "-Xfrontend",
            "Translation",
            "src/mac/services/translation.swift",
            "-o",
            &object,
        ])
        .status()
        .expect("xcrun swiftc is needed to build Mac translation support");
    assert!(status.success(), "Translation bridge did not compile");
    let status = Command::new("xcrun")
        .args(["ar", "crs", &library, &object])
        .status()
        .expect("xcrun ar is needed");
    assert!(status.success(), "Translation bridge did not archive");

    // Swift 对象携带运行库的自动链接项；链接器仍需要工具链与 SDK 的查找路径。
    let swift = Command::new("xcrun")
        .args(["--find", "swiftc"])
        .output()
        .expect("xcrun cannot locate swiftc");
    assert!(swift.status.success(), "xcrun cannot locate swiftc");
    let swift = String::from_utf8(swift.stdout).expect("swiftc path must be UTF-8");
    let toolchain = Path::new(swift.trim())
        .parent()
        .and_then(Path::parent)
        .expect("swiftc must be in a toolchain bin directory");
    let sdk = Command::new("xcrun")
        .arg("--show-sdk-path")
        .output()
        .expect("xcrun cannot locate the Mac SDK");
    assert!(sdk.status.success(), "xcrun cannot locate the Mac SDK");
    let sdk = String::from_utf8(sdk.stdout).expect("SDK path must be UTF-8");
    println!("cargo:rustc-link-search=native={out}");
    println!(
        "cargo:rustc-link-search=native={}",
        toolchain.join("lib/swift/macosx").display()
    );
    println!(
        "cargo:rustc-link-search=native={}/usr/lib/swift",
        sdk.trim()
    );
    println!("cargo:rustc-link-lib=static=poni_translation");
    // Translation.framework 自 macOS 15 才存在；弱链接让最低 14 的应用仍能启动。
    println!("cargo:rustc-link-arg-bins=-Wl,-weak_framework,Translation");
    println!("cargo:rustc-link-arg-bins=-Wl,-rpath,/usr/lib/swift");
}

/// 发行说明地址需要的构建来源；本地构建不携带CI提交。
fn build_info() {
    for var in [
        "GITHUB_ACTIONS",
        "GITHUB_REF_TYPE",
        "GITHUB_REF_NAME",
        "GITHUB_SHA",
        "PONI_RELEASE_BUILD",
    ] {
        println!("cargo:rerun-if-env-changed={var}");
    }
    let env = |k: &str| std::env::var(k).unwrap_or_default();
    let ci = env("GITHUB_ACTIONS") == "true";
    let release = ci
        && (env("PONI_RELEASE_BUILD") == "true"
            || (env("GITHUB_REF_TYPE") == "tag"
                && env("GITHUB_REF_NAME") == format!("v{}", env("CARGO_PKG_VERSION"))));
    let kind = if release {
        "release"
    } else if ci {
        "test"
    } else {
        "local"
    };
    let commit: String = if ci {
        env("GITHUB_SHA").chars().take(7).collect()
    } else {
        String::new()
    };
    println!("cargo:rustc-env=PONI_BUILD_KIND={kind}");
    println!("cargo:rustc-env=PONI_BUILD_COMMIT={commit}");
}
