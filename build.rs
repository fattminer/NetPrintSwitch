// NetPrintSwitch
// Copyright (C) 2026 fattminer
// SPDX-License-Identifier: LicenseRef-NetPrintSwitch-AGPL-3.0-only-PLUS-Commons-Clause-1.0

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::Command,
};

fn main() {
    println!("cargo:rerun-if-changed=PrintSwitch.rc");
    println!("cargo:rerun-if-changed=PrintSwitch.ico");

    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let resource = out_dir.join("netprintswitch.res");
    let script = manifest_dir.join("PrintSwitch.rc");
    let rc = find_resource_compiler().expect("Windows SDK rc.exe not found");
    let output = format!("/fo{}", resource.display());
    let status = Command::new(rc)
        .args(["/nologo", &output])
        .arg(script)
        .status()
        .expect("failed to start Windows resource compiler");
    assert!(status.success(), "rc.exe failed with status {status}");

    println!(
        "cargo:rustc-link-arg-bin=netprintswitch={}",
        resource.display()
    );
}

fn find_resource_compiler() -> Option<PathBuf> {
    if let Some(sdk_dir) = env::var_os("WindowsSdkDir") {
        let root = PathBuf::from(sdk_dir);
        if let Some(path) = find_rc_in_bin(&root.join("bin")) {
            return Some(path);
        }
        if let Some(path) = find_rc_under(&root) {
            return Some(path);
        }
    }

    let program_files_x86 = env::var_os("ProgramFiles(x86)")?;
    let bin = PathBuf::from(program_files_x86).join("Windows Kits\\10\\bin");
    find_rc_in_bin(&bin).or_else(|| find_rc_under(&bin))
}

fn find_rc_in_bin(root: &Path) -> Option<PathBuf> {
    let mut versions: Vec<_> = fs::read_dir(root)
        .ok()?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .collect();
    versions.sort();
    versions.into_iter().rev().find_map(|version| {
        let path = version.join("x64\\rc.exe");
        path.is_file().then_some(path)
    })
}

fn find_rc_under(root: &Path) -> Option<PathBuf> {
    let mut matches = Vec::new();
    collect_rc_files(root, &mut matches);
    matches.sort();
    matches.into_iter().rev().find(|path| {
        path.parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("x64"))
    })
}

fn collect_rc_files(root: &Path, matches: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_rc_files(&path, matches);
        } else if path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("rc.exe"))
        {
            matches.push(path);
        }
    }
}
