fn new_cc_build() -> cc::Build {
    let mut b = cc::Build::new();
    #[cfg(target_vendor = "apple")]
    {
        b.flag("-mmacosx-version-min=12.0");
    }
    b
}

fn main() {
    #[cfg(target_vendor = "apple")]
    {
        std::env::set_var("MACOSX_DEPLOYMENT_TARGET", "12.0");
        println!("cargo:rustc-env=MACOSX_DEPLOYMENT_TARGET=12.0");
        println!("cargo:rustc-link-arg=-mmacosx-version-min=12.0");
    }

    #[cfg(feature = "napi")]
    napi_build::setup();

    // Kotlin grammar
    let mut c = new_cc_build();
    c.include("grammars/kotlin");
    c.file("grammars/kotlin/parser.c");
    c.file("grammars/kotlin/scanner.c");
    c.flag_if_supported("-Wno-unused-parameter");
    c.flag_if_supported("-Wno-unused-but-set-variable");
    c.flag_if_supported("-Wno-trigraphs");
    c.flag_if_supported("-utf-8"); // msvc
    c.compile("tree-sitter-kotlin");
    println!("cargo:rerun-if-changed=grammars/kotlin");

    // Lua grammar
    let mut lua = new_cc_build();
    lua.include("grammars/lua");
    lua.file("grammars/lua/parser.c");
    lua.file("grammars/lua/scanner.c");
    lua.flag_if_supported("-Wno-unused-parameter");
    lua.flag_if_supported("-Wno-unused-but-set-variable");
    lua.flag_if_supported("-Wno-trigraphs");
    lua.flag_if_supported("-utf-8"); // msvc
    lua.compile("tree-sitter-lua");
    println!("cargo:rerun-if-changed=grammars/lua");

    // Scala grammar
    let mut scala = new_cc_build();
    scala.include("grammars/scala");
    scala.file("grammars/scala/parser.c");
    scala.file("grammars/scala/scanner.c");
    scala.flag_if_supported("-Wno-unused-parameter");
    scala.flag_if_supported("-Wno-unused-but-set-variable");
    scala.flag_if_supported("-Wno-trigraphs");
    scala.flag_if_supported("-utf-8"); // msvc
    scala.compile("tree-sitter-scala");
    println!("cargo:rerun-if-changed=grammars/scala");

    // Dart grammar
    let mut dart = new_cc_build();
    dart.include("grammars/dart");
    dart.file("grammars/dart/parser.c");
    dart.file("grammars/dart/scanner.c");
    dart.flag_if_supported("-Wno-unused-parameter");
    dart.flag_if_supported("-Wno-unused-but-set-variable");
    dart.flag_if_supported("-Wno-trigraphs");
    dart.flag_if_supported("-Wno-unused-function");
    dart.flag_if_supported("-utf-8"); // msvc
    dart.compile("tree-sitter-dart");
    println!("cargo:rerun-if-changed=grammars/dart");

    // Additional 12 vendored C grammars
    let grammars: &[(&str, &str, &[&str], &[&str])] = &[
        ("tree-sitter-arkts", "grammars/arkts/src", &["parser.c", "scanner.c"], &[]),
        ("tree-sitter-hcl", "grammars/terraform", &["parser.c", "scanner.c"], &[]),
        ("tree-sitter-objc", "grammars/objc", &["parser.c"], &[]),
        ("tree-sitter-solidity", "grammars/solidity", &["parser.c"], &[]),
        ("tree-sitter-erlang", "grammars/erlang", &["parser.c", "scanner.c"], &[]),
        ("tree-sitter-pascal", "grammars/pascal", &["parser.c"], &[]),
        ("tree-sitter-cfml", "grammars/cfml_family/cfml/src", &["parser.c", "scanner.c"], &["-std=c11"]),
        ("tree-sitter-cfscript", "grammars/cfml_family/cfscript/src", &["parser.c", "scanner.c"], &["-std=c11"]),
        ("tree-sitter-cfquery", "grammars/cfml_family/cfquery/src", &["parser.c", "scanner.c"], &["-std=c11"]),
        ("tree-sitter-nix", "grammars/nix", &["parser.c", "scanner.c"], &[]),
        ("tree-sitter-cobol", "grammars/cobol", &["parser.c", "scanner.c"], &[]),
        ("tree-sitter-vbnet", "grammars/vbnet", &["parser.c", "scanner.c"], &[]),
    ];

    for &(lib_name, dir, files, extra_flags) in grammars {
        let mut build = new_cc_build();
        build.include(dir);
        for &file in files {
            build.file(format!("{dir}/{file}"));
        }
        build.flag_if_supported("-Wno-unused-parameter");
        build.flag_if_supported("-Wno-unused-but-set-variable");
        build.flag_if_supported("-Wno-trigraphs");
        build.flag_if_supported("-Wno-unused-function");
        build.flag_if_supported("-Wno-unused-variable");
        build.flag_if_supported("-Wno-implicit-fallthrough");
        build.flag_if_supported("-utf-8"); // msvc
        for &flag in extra_flags {
            build.flag_if_supported(flag);
        }
        build.compile(lib_name);
        println!("cargo:rerun-if-changed={dir}");
    }
}
