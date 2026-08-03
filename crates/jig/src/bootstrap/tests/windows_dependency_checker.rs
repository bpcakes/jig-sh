use super::*;
use std::process::{Command, Output, Stdio};

const METADATA_LAUNCHER_BEGIN: &str = "// BEGIN JIG PACKAGE MANAGER METADATA LAUNCHER\n";
const METADATA_LAUNCHER_END: &str = "// END JIG PACKAGE MANAGER METADATA LAUNCHER";
const PNPM_MANIFEST_LAUNCHER_BEGIN: &str = "// BEGIN JIG PNPM MANIFEST QUERY LAUNCHER\n";
const PNPM_MANIFEST_LAUNCHER_END: &str = "// END JIG PNPM MANIFEST QUERY LAUNCHER";

fn render_dependency_checker(package_manager: &str) -> (TempDir, PathBuf) {
    let temp = tempdir().unwrap();
    let template = materialize_template_worktree();
    let repo_name = format!("windows-{package_manager}-dependency-checker");
    let repo = temp.path().join(&repo_name);
    run_init(InitOpts {
        path: repo.clone(),
        scaffold: ScaffoldOpts::default(),
        template: Some(template.path().display().to_string()),
        template_mode: None,
        vcs_ref: None,
        force: false,
        defaults: true,
        no_input: true,
        no_vault: true,
        answers: AnswerOpts {
            repo_name: Some(repo_name),
            sqlx_enabled: Some(false),
            web_package_manager: Some(package_manager.into()),
            frontend_apps: vec![FrontendApp {
                name: "web".into(),
                dir: "apps/web".into(),
                coverage_threshold: 80,
                kind: "vite".into(),
                role: "spa".into(),
            }],
            ..AnswerOpts::default()
        },
    })
    .unwrap();
    (temp, repo)
}

#[cfg(unix)]
fn render_npm_dependency_checker() -> (TempDir, PathBuf) {
    render_dependency_checker("npm")
}

fn metadata_launcher_blocks(script: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut remainder = script;
    while let Some(start) = remainder.find(METADATA_LAUNCHER_BEGIN) {
        let marked = &remainder[start..];
        let end = marked
            .find(METADATA_LAUNCHER_END)
            .unwrap_or_else(|| panic!("metadata launcher is missing its end marker"))
            + METADATA_LAUNCHER_END.len();
        blocks.push(marked[..end].to_owned());
        remainder = &marked[end..];
    }
    blocks
}

fn command_interpreter_validators(script: &str) -> Vec<String> {
    const BEGIN: &str = "function validateWindowsCommandInterpreter(interpreter) {\n";
    const END: &str = "  return interpreter;\n}";

    let mut blocks = Vec::new();
    let mut remainder = script;
    while let Some(start) = remainder.find(BEGIN) {
        let marked = &remainder[start..];
        let end = marked
            .find(END)
            .unwrap_or_else(|| panic!("command-interpreter validator is incomplete"))
            + END.len();
        blocks.push(marked[..end].to_owned());
        remainder = &marked[end..];
    }
    blocks
}

fn marked_launcher(script: &str, begin: &str, end: &str) -> String {
    let start = script
        .find(begin)
        .unwrap_or_else(|| panic!("launcher is missing its begin marker"));
    let marked = &script[start..];
    let finish = marked
        .find(end)
        .unwrap_or_else(|| panic!("launcher is missing its end marker"))
        + end.len();
    marked[..finish].to_owned()
}

fn run_node_harness(source: &str, arguments: &[&Path]) -> Output {
    let mut child = Command::new("node")
        .arg("-")
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(source.as_bytes())
        .unwrap();
    child.wait_with_output().unwrap()
}

fn shell_function_between(script: &str, name: &str, next_name: &str) -> String {
    let start_marker = format!("{name}() {{\n");
    let end_marker = format!("\n{next_name}() {{\n");
    let start = script
        .find(&start_marker)
        .unwrap_or_else(|| panic!("rendered checker is missing {name}"));
    let end = script[start..]
        .find(&end_marker)
        .map(|offset| start + offset + 1)
        .unwrap_or_else(|| panic!("rendered checker is missing {next_name} after {name}"));
    script[start..end].to_owned()
}

#[cfg(unix)]
fn run_bash_harness(script: &str) -> std::process::Output {
    std::process::Command::new("bash")
        .arg("-c")
        .arg(script)
        .output()
        .unwrap()
}

#[test]
fn generated_metadata_launchers_are_identical_and_cover_every_probe() {
    let _guard = lock_env();
    let (_pnpm_temp, pnpm_repo) = render_dependency_checker("pnpm");
    let (_yarn_temp, yarn_repo) = render_dependency_checker("yarn");
    let pnpm = fs::read_to_string(pnpm_repo.join("scripts/check-webapps.sh")).unwrap();
    let yarn = fs::read_to_string(yarn_repo.join("scripts/check-webapps.sh")).unwrap();
    let pnpm_helper = fs::read_to_string(pnpm_repo.join("scripts/web-node.cjs")).unwrap();
    let yarn_helper = fs::read_to_string(yarn_repo.join("scripts/web-node.cjs")).unwrap();

    let pnpm_launchers = metadata_launcher_blocks(&pnpm_helper);
    let yarn_launchers = metadata_launcher_blocks(&yarn_helper);
    assert_eq!(pnpm_launchers.len(), 4, "pnpm must render every launcher");
    assert_eq!(yarn_launchers.len(), 3, "Yarn must render every launcher");
    for launcher in pnpm_launchers.iter().chain(&yarn_launchers) {
        assert_eq!(launcher, &pnpm_launchers[0]);
        assert!(launcher.contains(r#"["config", "list", "--json"]"#));
        assert!(launcher.contains("timeout: 30_000, ...options, shell: false"));
        assert!(!launcher.contains(r#"["config", "get""#));
    }

    let validators = command_interpreter_validators(&pnpm_helper)
        .into_iter()
        .chain(command_interpreter_validators(&yarn_helper))
        .collect::<Vec<_>>();
    assert_eq!(validators.len(), 11);
    for validator in &validators {
        assert_eq!(validator, &validators[0]);
    }
    for checker in [&pnpm_helper, &yarn_helper] {
        assert!(!checker.contains(r#""COMSPEC") || "cmd.exe""#));
        assert!(!checker.contains(r#""COMSPEC") || 'cmd.exe'"#));
    }

    let pnpm_workspace =
        shell_function_between(&pnpm, "workspace_metadata", "root_workspace_contains_app");
    let pnpm_runtime =
        shell_function_between(&pnpm, "pnpm_runtime_snapshot", "pnpm_encode_contract");
    let pnpm_spec = shell_function_between(
        &pnpm,
        "pnpm_package_manager_spec_for_scope",
        "pnpm_runtime_snapshot",
    );
    let yarn_classic = shell_function_between(
        &yarn,
        "yarn_classic_config_payload",
        "yarn_classic_actual_artifact_kind",
    );
    let yarn_berry = shell_function_between(
        &yarn,
        "yarn_berry_config_payload",
        "yarn_berry_config_value",
    );
    let yarn_runtime =
        shell_function_between(&yarn, "yarn_runtime_identity", "dependency_lockfile");
    assert!(pnpm_workspace.contains("run_web_node workspace_metadata"));
    assert!(pnpm_runtime.contains("run_web_node pnpm_runtime_snapshot"));
    assert!(pnpm_spec.contains("run_web_node pnpm_package_manager_spec_for_scope"));
    assert!(yarn_classic.contains("run_web_node yarn_classic_config_payload"));
    assert!(yarn_berry.contains("run_web_node yarn_berry_config_payload"));
    assert!(yarn_runtime.contains("run_web_node yarn_runtime_identity"));

    for checker in [&pnpm_helper, &yarn_helper] {
        assert!(!checker.contains("shell: true"));
        assert!(!checker.contains("shell: process.platform"));
        assert!(!checker.contains("spawnSync(\"pnpm\""));
        assert!(!checker.contains("spawnSync(\"yarn\""));
    }
    assert!(pnpm_helper.contains("const result = spawnPackageManagerMetadata(\n        \"pnpm\","));
    assert!(pnpm_helper.contains("spawnPackageManagerMetadata(executable.path, args, {"));
    assert!(yarn_helper.contains("spawnPackageManagerMetadata(\"yarn\", args, {"));
    assert!(
        yarn_helper.contains("spawnPackageManagerMetadata(\"yarn\", [\"config\", \"--json\"], {")
    );
    assert!(yarn_helper.contains("environmentValue(\"PATHEXT\")"));
    assert!(yarn_helper.contains("windowsPathEntries(pathValue)"));
    assert!(yarn_helper.contains("spawnSync(resolved, [\"--version\"], options)"));
    assert!(
        yarn_helper.contains("validateWindowsCommandInterpreter(environmentValue(\"COMSPEC\"))")
    );
    assert!(yarn_helper.contains("fs.openSync(resolved, \"r\")"));
    assert!(!yarn_helper.contains("type -P yarn"));
    assert!(!yarn_helper.contains("yarn --version"));
}

#[test]
fn metadata_launcher_quotes_windows_wrappers_and_rejects_untrusted_inputs() {
    let _guard = lock_env();
    let (_temp, repo) = render_dependency_checker("pnpm");
    let checker = fs::read_to_string(repo.join("scripts/web-node.cjs")).unwrap();
    let launcher = metadata_launcher_blocks(&checker).remove(0);
    let fixture = tempdir().unwrap();
    let command_wrapper = fixture.path().join("package manager & shim.CMD");
    let native_executable = fixture.path().join("package manager & native.EXE");
    let explicit_denied = fixture.path().join("explicit denied.CMD");
    let percent_wrapper = fixture.path().join("package % manager.CMD");
    let path_delimiter = if cfg!(windows) { ';' } else { ':' };
    let quoted_directory = fixture
        .path()
        .join(format!("quoted{path_delimiter}directory"));
    fs::create_dir_all(&quoted_directory).unwrap();
    let quoted_wrapper = quoted_directory.join("quoted shim.CMD");
    fs::write(&command_wrapper, "wrapper").unwrap();
    fs::write(&native_executable, "native").unwrap();
    fs::write(&percent_wrapper, "wrapper").unwrap();
    fs::write(&quoted_wrapper, "wrapper").unwrap();

    let harness = format!(
        r#"const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const calls = [];
function spawnSync(command, args, options) {{
  calls.push({{ command, args, options }});
  return {{ status: 0, signal: null, error: null, stdout: "11.13.0\n" }};
}}
{launcher}

Object.defineProperty(process, "platform", {{ value: "linux" }});
const direct = spawnPackageManagerMetadata("yarn", ["--version"], {{
  encoding: "utf8",
  env: process.env,
  maxBuffer: 8192,
  timeout: 30000,
}});
assert.equal(direct.stdout, "11.13.0\n");
assert.equal(calls[0].command, "yarn");
assert.deepEqual(calls[0].args, ["--version"]);
assert.equal(calls[0].options.shell, false);

Object.defineProperty(process, "platform", {{ value: "win32" }});
const environment = {{ ...process.env, ComSpec: "C:\\Windows\\System32\\cmd.exe" }};
const wrapperResult = spawnPackageManagerMetadata(process.argv[2], ["--version"], {{
  cwd: process.cwd(),
  encoding: "utf8",
  env: environment,
  maxBuffer: 16384,
  timeout: 30000,
}});
assert.equal(wrapperResult.stdout, "11.13.0\n");
const wrapperCall = calls.at(-1);
const resolvedWrapper = fs.realpathSync.native(process.argv[2]);
assert.equal(wrapperCall.command, environment.ComSpec);
assert.deepEqual(wrapperCall.args.slice(0, 4), ["/d", "/s", "/v:off", "/c"]);
assert.equal(wrapperCall.args[4], `""${{resolvedWrapper}}" --version"`);
assert.equal(wrapperCall.options.shell, false);
assert.equal(wrapperCall.options.windowsVerbatimArguments, true);
assert.equal(wrapperCall.options.timeout, 30000);
assert.equal(wrapperCall.options.maxBuffer, 16384);
assert.equal(wrapperCall.options.env, environment);

const callsBeforeInterpreterRejections = calls.length;
for (const badEnvironment of [
  {{}},
  {{ ComSpec: "cmd.exe" }},
  {{ ComSpec: String.raw`C:cmd.exe` }},
  {{ ComSpec: String.raw`\Windows\System32\cmd.exe` }},
  {{ ComSpec: String.raw`\\?\C:\Windows\System32\cmd.exe` }},
]) {{
  assert.throws(
    () => spawnPackageManagerMetadata(process.argv[2], ["--version"], {{ env: badEnvironment }}),
    /unsafe Windows command interpreter/
  );
}}
assert.equal(calls.length, callsBeforeInterpreterRejections);

const uncEnvironment = {{
  ...environment,
  ComSpec: String.raw`\\server\share\cmd.exe`,
}};
spawnPackageManagerMetadata(process.argv[2], ["--version"], {{ env: uncEnvironment }});
assert.equal(calls.at(-1).command, uncEnvironment.ComSpec);

const bareEnvironment = {{
  ...environment,
  PATH: path.dirname(process.argv[2]),
  PATHEXT: ".CMD",
}};
const bareResult = spawnPackageManagerMetadata("package manager & shim", ["--version"], {{
  encoding: "utf8",
  env: bareEnvironment,
  maxBuffer: 2048,
  timeout: 4321,
}});
assert.equal(bareResult.stdout, "11.13.0\n");
const bareCall = calls.at(-1);
assert.equal(bareCall.command, environment.ComSpec);
assert.equal(bareCall.args[4], `""${{resolvedWrapper}}" --version"`);
assert.equal(bareCall.options.windowsVerbatimArguments, true);

const originalStatSync = fs.statSync;
const inaccessibleDirectory = path.join(path.dirname(process.argv[2]), "inaccessible");
const deadNetworkDirectory = path.join(path.dirname(process.argv[2]), "dead-network");
fs.statSync = function(candidate, ...args) {{
  if (candidate.startsWith(inaccessibleDirectory)) {{
    const error = new Error("access denied");
    error.code = "EACCES";
    throw error;
  }}
  if (candidate.startsWith(deadNetworkDirectory)) {{
    const error = new Error("network path unavailable");
    error.code = "EHOSTUNREACH";
    throw error;
  }}
  if (candidate === process.argv[4]) {{
    const error = new Error("explicit access denied");
    error.code = "EACCES";
    throw error;
  }}
  return originalStatSync.call(fs, candidate, ...args);
}};
try {{
  const resilientEnvironment = {{
    ...environment,
    PATH: [inaccessibleDirectory, deadNetworkDirectory, path.dirname(process.argv[2])].join(path.delimiter),
    PATHEXT: ".CMD",
  }};
  spawnPackageManagerMetadata("package manager & shim", ["--version"], {{
    encoding: "utf8",
    env: resilientEnvironment,
    maxBuffer: 2048,
    timeout: 4321,
  }});
  assert.equal(calls.at(-1).args[4], `""${{resolvedWrapper}}" --version"`);
  assert.throws(
    () => spawnPackageManagerMetadata(process.argv[4], ["--version"], {{ env: environment }}),
    /explicit access denied/
  );
}} finally {{
  fs.statSync = originalStatSync;
}}

const callsBeforeRejections = calls.length;
assert.throws(
  () => spawnPackageManagerMetadata(process.argv[2], ["--version", "&whoami"], {{ env: environment }}),
  /unsupported package-manager metadata arguments/
);
for (const executable of [
  `${{process.argv[2]}}"suffix`,
  `${{process.argv[2]}}\ncontrol`,
]) {{
  assert.throws(
    () => spawnPackageManagerMetadata(executable, ["--version"], {{ env: environment }}),
    /unsafe package-manager metadata executable/
  );
}}
assert.equal(calls.length, callsBeforeRejections);

spawnPackageManagerMetadata(process.argv[5], ["--version"], {{ env: environment }});
const percentCall = calls.at(-1);
assert.equal(percentCall.command, environment.ComSpec);
assert.match(percentCall.args[4], /%%cd:~,%/);

spawnPackageManagerMetadata(process.argv[3], ["config", "--json"], {{
  encoding: "utf8",
  env: environment,
  maxBuffer: 4096,
  timeout: 1234,
}});
const nativeCall = calls.at(-1);
assert.equal(nativeCall.command, fs.realpathSync.native(process.argv[3]));
assert.deepEqual(nativeCall.args, ["config", "--json"]);
assert.equal(nativeCall.options.shell, false);
assert.equal(nativeCall.options.windowsVerbatimArguments, undefined);
assert.equal(nativeCall.options.timeout, 1234);
assert.equal(nativeCall.options.maxBuffer, 4096);

spawnPackageManagerMetadata(process.argv[3], ["config", "list", "--json"], {{
  encoding: "utf8",
  env: environment,
}});
const configListCall = calls.at(-1);
assert.equal(configListCall.command, fs.realpathSync.native(process.argv[3]));
assert.deepEqual(configListCall.args, ["config", "list", "--json"]);
assert.equal(configListCall.options.shell, false);

assert.throws(
  () => spawnPackageManagerMetadata("package manager & shim", ["--version"], {{
    env: {{ ...bareEnvironment, PATHEXT: ".CMD;.BAD!" }},
  }}),
  /unsafe Windows PATHEXT/
);

spawnPackageManagerMetadata("quoted shim", ["--version"], {{
  env: {{ ...environment, PATH: `"${{path.dirname(process.argv[6])}}"`, PATHEXT: ".CMD" }},
}});
assert.equal(calls.at(-1).args[4], `""${{fs.realpathSync.native(process.argv[6])}}" --version"`);

const relativeDirectory = path.relative(process.cwd(), path.dirname(process.argv[2]));
spawnPackageManagerMetadata("package manager & shim", ["--version"], {{
  cwd: process.cwd(),
  env: {{ ...environment, PATH: relativeDirectory, PATHEXT: ".CMD" }},
}});
assert.equal(calls.at(-1).args[4], `""${{resolvedWrapper}}" --version"`);
"#
    );
    let output = run_node_harness(
        &harness,
        &[
            &command_wrapper,
            &native_executable,
            &explicit_denied,
            &percent_wrapper,
            &quoted_wrapper,
        ],
    );
    assert!(
        output.status.success(),
        "metadata launcher harness failed with {:?}:\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn pnpm_manifest_parser_launcher_quotes_corepack_cmd_and_allows_only_the_fixed_query() {
    let _guard = lock_env();
    let (_temp, repo) = render_dependency_checker("pnpm");
    let checker = fs::read_to_string(repo.join("scripts/web-node.cjs")).unwrap();
    let launcher = marked_launcher(
        &checker,
        PNPM_MANIFEST_LAUNCHER_BEGIN,
        PNPM_MANIFEST_LAUNCHER_END,
    );
    let fixture = tempdir().unwrap();
    let corepack = fixture.path().join("core pack & shim.CMD");
    fs::write(&corepack, "wrapper").unwrap();
    let harness = format!(
        r#"const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const fallbackSpec = "pnpm@11.13.0";
const calls = [];
function spawnSync(command, args, options) {{
  calls.push({{ command, args, options }});
  return {{ status: 0, signal: null, error: null, stdout: "{{}}\n" }};
}}
function fail(message) {{ throw new Error(message); }}
function plainObject(value) {{ return typeof value === "object" && value !== null && !Array.isArray(value); }}
{launcher}

Object.defineProperty(process, "platform", {{ value: "win32" }});
const environment = {{ ...process.env, ComSpec: "C:\\Windows\\System32\\cmd.exe" }};
const query = [fallbackSpec, "pkg", "get", "packageManager", "devEngines.packageManager", "--json", "--ignore-workspace"];
spawnManifestParser(fs.realpathSync.native(process.argv[2]), query, {{
  cwd: process.cwd(),
  encoding: "utf8",
  env: environment,
  maxBuffer: 16384,
  timeout: 30000,
}});
const call = calls.at(-1);
assert.equal(call.command, environment.ComSpec);
assert.deepEqual(call.args.slice(0, 4), ["/d", "/s", "/v:off", "/c"]);
assert.match(call.args[4], /pnpm@11\.13\.0 pkg get packageManager devEngines\.packageManager --json/);
assert.equal(call.options.shell, false);
assert.equal(call.options.windowsVerbatimArguments, true);
assert.equal(call.options.timeout, 30000);
assert.equal(call.options.maxBuffer, 16384);

const callsBeforeInterpreterRejections = calls.length;
for (const badEnvironment of [{{}}, {{ ComSpec: "cmd.exe" }}, {{ ComSpec: String.raw`C:cmd.exe` }}]) {{
  assert.throws(
    () => spawnManifestParser(fs.realpathSync.native(process.argv[2]), query, {{ env: badEnvironment }}),
    /unsafe Windows command interpreter/
  );
}}
assert.equal(calls.length, callsBeforeInterpreterRejections);

const uncEnvironment = {{ ...environment, ComSpec: String.raw`\\server\share\cmd.exe` }};
spawnManifestParser(fs.realpathSync.native(process.argv[2]), query, {{ env: uncEnvironment }});
assert.equal(calls.at(-1).command, uncEnvironment.ComSpec);

const callsBeforeRejection = calls.length;
assert.throws(
  () => spawnManifestParser(fs.realpathSync.native(process.argv[2]), [fallbackSpec, "install"], {{ env: environment }}),
  /unsupported pnpm manifest-parser arguments/
);
assert.equal(calls.length, callsBeforeRejection);
"#
    );
    let output = run_node_harness(&harness, &[&corepack]);
    assert!(
        output.status.success(),
        "pnpm manifest launcher harness failed with {:?}:\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(windows)]
#[test]
fn metadata_launcher_executes_a_real_windows_cmd_wrapper() {
    if !Command::new("node")
        .arg("--version")
        .status()
        .is_ok_and(|status| status.success())
    {
        eprintln!("skipping .CMD smoke test because node is unavailable");
        return;
    }

    let _guard = lock_env();
    let (_temp, repo) = render_dependency_checker("pnpm");
    let checker = fs::read_to_string(repo.join("scripts/check-webapps.sh")).unwrap();
    let launcher = metadata_launcher_blocks(&checker).remove(0);
    let fixture = tempdir().unwrap();
    let command_wrapper = fixture.path().join("package manager & smoke.CMD");
    fs::write(
        &command_wrapper,
        "@echo off\r\nif not \"%~1\"==\"--version\" exit /b 41\r\necho 11.13.0\r\n",
    )
    .unwrap();
    let harness = format!(
        r#"const assert = require("node:assert/strict");
const {{ spawnSync }} = require("node:child_process");
{launcher}
const environmentWithoutComSpec = Object.fromEntries(
  Object.entries(process.env).filter(([name]) => name.toUpperCase() !== "COMSPEC")
);
assert.throws(
  () => spawnPackageManagerMetadata(process.argv[2], ["--version"], {{
    env: environmentWithoutComSpec,
  }}),
  /unsafe Windows command interpreter/
);
assert.throws(
  () => spawnPackageManagerMetadata(process.argv[2], ["--version"], {{
    env: {{ ...environmentWithoutComSpec, ComSpec: "cmd.exe" }},
  }}),
  /unsafe Windows command interpreter/
);
const result = spawnPackageManagerMetadata(process.argv[2], ["--version"], {{
  cwd: process.cwd(),
  encoding: "utf8",
  env: process.env,
  maxBuffer: 16384,
  timeout: 30000,
}});
if (result.error || result.signal || result.status !== 0) {{
  console.error(result.error || result.stderr || `status ${{result.status}}`);
  process.exit(1);
}}
process.stdout.write(result.stdout);
"#
    );
    let output = run_node_harness(&harness, &[&command_wrapper]);
    assert!(
        output.status.success(),
        "real .CMD launcher smoke test failed with {:?}:\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "11.13.0");
}

#[cfg(unix)]
#[test]
fn generated_dependency_checker_validates_procfs_identity_fields() {
    let _guard = lock_env();
    let (_temp, repo) = render_npm_dependency_checker();
    let checker = fs::read_to_string(repo.join("scripts/check-webapps.sh")).unwrap();

    let syntax = std::process::Command::new("bash")
        .args(["-n", "scripts/check-webapps.sh"])
        .current_dir(&repo)
        .status()
        .unwrap();
    assert!(
        syntax.success(),
        "rendered dependency checker is not valid Bash"
    );

    assert!(
        checker.contains(
            "printf 'linux.%s.%s.%s\\n' \"$boot_id\" \"$pid_namespace\" \"$start_ticks\""
        )
    );
    assert!(checker.contains("readlink \"/proc/$pid/ns/pid\""));
    assert!(checker.contains("printf 'cygwin-msys.%s.%s\\n' \"$boot_time\" \"$start_ticks\""));
    assert!(checker.contains("TZ=UTC0 LC_ALL=C ps -p \"$pid\" -o lstart="));
    assert!(checker.contains("[ -e \"/proc/$pid/pgid\" ]"));
    assert!(checker.contains("worker_group=\"$(process_group_id \"$install_worker_pid\")\""));
    assert!(checker.contains("worker_start=\"$(process_start_identity \"$$\")\""));
    assert!(checker.contains("[ \"$owner_start\" = \"${worker_start}.g$$\" ]"));
    assert!(!checker.contains("worker_group=\"$(ps -p \"$install_worker_pid\" -o pgid="));

    let stat_parser = shell_function_between(
        &checker,
        "process_stat_start_ticks",
        "process_stat_group_id",
    );
    let group_parser = shell_function_between(
        &checker,
        "process_stat_group_id",
        "procfs_boot_time_from_stat",
    );
    let boot_time_parser =
        shell_function_between(&checker, "procfs_boot_time_from_stat", "procfs_boot_time");
    let integer_parser = shell_function_between(
        &checker,
        "validated_positive_integer",
        "process_start_identity",
    );
    let harness = format!(
        r#"set -euo pipefail
{stat_parser}
{group_parser}
{boot_time_parser}
{integer_parser}
valid_stat='4242 (worker ) with spaces) S 1 31337 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 987654 20'
[ "$(process_stat_start_ticks 4242 "$valid_stat")" = 987654 ]
[ "$(process_stat_group_id 4242 "$valid_stat")" = 31337 ]
if process_stat_start_ticks 4243 "$valid_stat" >/dev/null 2>&1; then exit 11; fi
if process_stat_start_ticks 0 '0 (worker) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19' >/dev/null 2>&1; then exit 26; fi
if process_stat_start_ticks 4242 '4242 worker S 1 2 3' >/dev/null 2>&1; then exit 12; fi
if process_stat_start_ticks 4242 '4242 (worker) 9 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19' >/dev/null 2>&1; then exit 13; fi
if process_stat_start_ticks 4242 '4242 (worker) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 nope' >/dev/null 2>&1; then exit 14; fi
if process_stat_start_ticks 4242 $'4242 (worker) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19\nextra' >/dev/null 2>&1; then exit 15; fi
if process_stat_start_ticks 4242 '4242 (worker) S 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 0' >/dev/null 2>&1; then exit 22; fi
if process_stat_group_id 4243 "$valid_stat" >/dev/null 2>&1; then exit 23; fi
if process_stat_group_id 0 '0 (worker) S 1 2 3' >/dev/null 2>&1; then exit 27; fi
if process_stat_group_id 4242 '4242 (worker) S 1 nope 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19' >/dev/null 2>&1; then exit 24; fi
if process_stat_group_id 4242 '4242 (worker) S 1 0 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19' >/dev/null 2>&1; then exit 25; fi
[ "$(procfs_boot_time_from_stat $'cpu  1 2 3\nbtime 1712345678\nprocesses 9')" = 1712345678 ]
if procfs_boot_time_from_stat $'btime 1\nbtime 2' >/dev/null 2>&1; then exit 16; fi
if procfs_boot_time_from_stat 'btime 0' >/dev/null 2>&1; then exit 17; fi
if procfs_boot_time_from_stat 'btime 123x' >/dev/null 2>&1; then exit 18; fi
if procfs_boot_time_from_stat 'cpu 1 2 3' >/dev/null 2>&1; then exit 19; fi
[ "$(validated_positive_integer 0042)" = 0042 ]
if validated_positive_integer 0 >/dev/null 2>&1; then exit 20; fi
if validated_positive_integer -1 >/dev/null 2>&1; then exit 21; fi
"#
    );
    let output = run_bash_harness(&harness);
    assert!(
        output.status.success(),
        "procfs parser harness failed with {:?}:\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn generated_dependency_checker_never_classifies_cross_namespace_pid_as_stale() {
    let _guard = lock_env();
    let (_temp, repo) = render_npm_dependency_checker();
    let checker = fs::read_to_string(repo.join("scripts/check-webapps.sh")).unwrap();
    let namespace_parser = shell_function_between(
        &checker,
        "linux_identity_pid_namespace",
        "process_identity_state",
    );
    let identity_state =
        shell_function_between(&checker, "process_identity_state", "install_lock_state");
    let harness = format!(
        r#"set -euo pipefail
{namespace_parser}
{identity_state}
actual_identity=linux.boot-a.200.42
process_liveness_state() {{ return 0; }}
process_start_identity() {{ printf '%s\n' "$actual_identity"; }}
process_identity_state 77 linux.boot-a.200.42

set +e
process_identity_state 77 linux.boot-a.100.42
cross_namespace=$?
process_liveness_state() {{ return 1; }}
process_identity_state 77 linux.boot-a.100.42
cross_namespace_without_local_pid=$?
process_liveness_state() {{ return 0; }}
process_identity_state 77 linux.boot-a.42
legacy_identity=$?
process_identity_state 77 linux.boot-b.200.42
same_namespace_boot_mismatch=$?
process_identity_state 77 linux.boot-a.200.41
same_namespace_start_mismatch=$?
set -e
[ "$cross_namespace" -eq 2 ]
[ "$cross_namespace_without_local_pid" -eq 2 ]
[ "$legacy_identity" -eq 2 ]
[ "$same_namespace_boot_mismatch" -eq 1 ]
[ "$same_namespace_start_mismatch" -eq 1 ]
"#
    );
    let output = run_bash_harness(&harness);
    assert!(
        output.status.success(),
        "PID-namespace identity harness failed with {:?}:\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn generated_dependency_checker_uses_shell_procfs_before_native_node() {
    let _guard = lock_env();
    let (_temp, repo) = render_npm_dependency_checker();
    let checker = fs::read_to_string(repo.join("scripts/check-webapps.sh")).unwrap();

    let stat_parser = shell_function_between(
        &checker,
        "process_stat_start_ticks",
        "process_stat_group_id",
    );
    let group_parser = shell_function_between(
        &checker,
        "process_stat_group_id",
        "procfs_boot_time_from_stat",
    );
    let boot_time_parser =
        shell_function_between(&checker, "procfs_boot_time_from_stat", "procfs_boot_time");
    let boot_time_reader =
        shell_function_between(&checker, "procfs_boot_time", "validated_positive_integer");
    let integer_parser = shell_function_between(
        &checker,
        "validated_positive_integer",
        "process_start_identity",
    );
    let group_reader =
        shell_function_between(&checker, "process_group_id", "process_liveness_state");
    let liveness =
        shell_function_between(&checker, "process_liveness_state", "process_identity_state");
    let harness = format!(
        r#"set -euo pipefail
{stat_parser}
{group_parser}
{boot_time_parser}
{boot_time_reader}
{integer_parser}
{group_reader}
{liveness}
node_probe_marker="${{TMPDIR:-/tmp}}/jig-node-probe.$$"
rm -f "$node_probe_marker"
trap 'rm -f "$node_probe_marker"' EXIT
node_probe() {{ : > "$node_probe_marker"; return 99; }}
node_bin=node_probe
is_cygwin_msys_shell() {{ return 0; }}
kill() {{ return 1; }}
set +e
process_liveness_state 0
zero_status=$?
set -e
[ "$zero_status" -eq 2 ]
if [ -r /proc/stat ] && [ -r "/proc/$$/stat" ]; then
  process_liveness_state "$$"
  set +e
  process_liveness_state 999999999
  stale_status=$?
  set -e
  [ "$stale_status" -eq 1 ]
else
  set +e
  process_liveness_state "$$"
  live_status=$?
  process_liveness_state 999999999
  stale_status=$?
  set -e
  [ "$live_status" -eq 2 ]
  [ "$stale_status" -eq 2 ]
fi
[ ! -e "$node_probe_marker" ]
unset -f kill
group="$(process_group_id "$$")"
case "$group" in ''|*[!0-9]*|0) exit 31 ;; esac
"#
    );
    let output = run_bash_harness(&harness);
    assert!(
        output.status.success(),
        "MSYS/Cygwin ownership harness failed with {:?}:\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[cfg(unix)]
#[test]
fn generated_dependency_checker_only_recovers_exactly_stale_process_groups() {
    let _guard = lock_env();
    let (_temp, repo) = render_npm_dependency_checker();
    let checker = fs::read_to_string(repo.join("scripts/check-webapps.sh")).unwrap();
    let helper = fs::read_to_string(repo.join("scripts/web-node.cjs")).unwrap();

    let integer_parser = shell_function_between(
        &checker,
        "validated_positive_integer",
        "process_start_identity",
    );
    let group_liveness = shell_function_between(
        &checker,
        "process_group_liveness_state",
        "process_identity_state",
    );
    let install_lock_state =
        shell_function_between(&checker, "install_lock_state", "install_lock_is_stale");
    let acquire_lock = shell_function_between(
        &checker,
        "acquire_install_lock",
        "transfer_install_lock_to_worker",
    );

    assert!(install_lock_state.contains("process_group_liveness_state \"$owner_group\""));
    assert!(helper.contains("error?.code === \"ESRCH\""));
    assert!(
        !acquire_lock.contains("dependencies_present"),
        "install-lock polling must not rerun the full dependency proof"
    );

    let harness = format!(
        r#"set -euo pipefail
{integer_parser}
{group_liveness}
probe_result=unverified
probe_status=0
node_calls=0
node_probe() {{
  node_calls=$((node_calls + 1))
  printf '%s\n' "$probe_result"
  return "$probe_status"
}}
node_bin=node_probe
run_web_node() {{ shift; "$node_bin" "$@"; }}
is_cygwin_msys_shell() {{ return 1; }}
kill() {{ return 1; }}

set +e
process_group_liveness_state 0
invalid_status=$?
probe_result=stale
process_group_liveness_state 4242
stale_status=$?
probe_result=live
process_group_liveness_state 4242
live_status=$?
probe_result=unverified
process_group_liveness_state 4242
unverified_status=$?
probe_status=9
process_group_liveness_state 4242 >/dev/null
failed_probe_status=$?
set -e
[ "$invalid_status" -eq 2 ]
[ "$stale_status" -eq 1 ]
[ "$live_status" -eq 0 ]
[ "$unverified_status" -eq 2 ]
[ "$failed_probe_status" -eq 2 ]

probe_status=0
kill() {{ return 0; }}
process_group_liveness_state 4242

kill() {{ return 1; }}
is_cygwin_msys_shell() {{ return 0; }}
set +e
process_group_liveness_state 4242
msys_status=$?
set -e
[ "$msys_status" -eq 2 ]
"#
    );
    let output = run_bash_harness(&harness);
    assert!(
        output.status.success(),
        "process-group liveness harness failed with {:?}:\nstdout:\n{}\nstderr:\n{}",
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
