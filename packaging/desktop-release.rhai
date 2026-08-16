// Tracked copy of the Grok workflow (project .grok/ is gitignored).
// Install/run:
//   mkdir -p .grok/workflows
//   cp packaging/desktop-release.rhai .grok/workflows/desktop-release.rhai
//   /workflow desktop-release
// Or run the shell path directly:
//   ./packaging/make_desktop_release.sh

let meta = #{
    name: "desktop-release",
    description: "Compile desktop release, bundle, copy archive to release/ for download",
    when_to_use: "Ship a downloadable desktop build of vadadee-berry",
    phases: [
        #{ title: "Build", detail: "cargo release + package into release/" },
        #{ title: "Verify", detail: "confirm archive and checksums exist" },
    ],
};

let result_schema = #{
    "type": "object",
    "required": ["ok", "archive", "notes"],
    "properties": #{
        "ok": #{ "type": "boolean" },
        "archive": #{ "type": "string" },
        "notes": #{ "type": "string" },
    },
};

let no_build = false;
let bins = "all";
let no_opencv = false;
if args != () {
    if args.no_build == true { no_build = true; }
    if args.bins != () && args.bins != "" { bins = args.bins; }
    if args.no_opencv == true { no_opencv = true; }
}

phase("Build");
log("Running packaging/make_desktop_release.sh (host desktop release)");

let build_prompt = "";
build_prompt += "You are packaging Vadadee Berry for desktop download.\n";
build_prompt += "Working directory must be the repo root of vadadee-berry.\n";
build_prompt += "Run exactly (chmod +x if needed):\n\n";
build_prompt += "  ./packaging/make_desktop_release.sh";
if no_build {
    build_prompt += " --no-build";
}
build_prompt += " --bins " + bins;
if no_opencv {
    build_prompt += " --no-default-features";
}
build_prompt += "\n\n";
build_prompt += "This script compiles release binaries, bundles them under dist/desktop/, ";
build_prompt += "and copies the archive into the main project release/ folder for user download.\n";
build_prompt += "Do not skip the script. Capture stdout/stderr. If cargo fails, report the error.\n";
build_prompt += "When finished, list files under release/ with sizes.\n";

let build = agent(build_prompt, #{
    label: "desktop-release-build",
    capability_mode: "execute",
    output_schema: result_schema,
});

if build == () || !build.success {
    complete(#{
        summary: "Desktop release build failed or did not return a structured result",
        ok: false,
        archive: "",
        detail: build,
    });
}

phase("Verify");
let verify_prompt = "";
verify_prompt += "Verify the desktop release drop folder.\n";
verify_prompt += "1. List release/ (ls -lah release/).\n";
verify_prompt += "2. Confirm at least one .tar.gz or .zip exists with a non-zero size.\n";
verify_prompt += "3. If a .sha256 file exists, print it.\n";
verify_prompt += "4. Return ok=true only if a downloadable archive is present.\n";
verify_prompt += "5. Set archive to the primary versioned archive path (not the latest symlink).\n";
if build.output != () && build.output.archive != () {
    verify_prompt += "Build agent claimed archive: " + build.output.archive + "\n";
}

let verify = agent(verify_prompt, #{
    label: "desktop-release-verify",
    capability_mode: "execute",
    output_schema: result_schema,
});

let ok = false;
let archive = "";
let notes = "";
if verify != () && verify.success && verify.output != () {
    ok = verify.output.ok == true;
    if verify.output.archive != () { archive = verify.output.archive; }
    if verify.output.notes != () { notes = verify.output.notes; }
}

if !ok {
    complete(#{
        summary: "Release archive missing or verification failed",
        ok: false,
        archive: archive,
        notes: notes,
        build: build.output,
        verify: if verify != () { verify.output } else { () },
    });
}

complete(#{
    summary: "Desktop release ready for download in release/",
    ok: true,
    archive: archive,
    notes: notes,
});
