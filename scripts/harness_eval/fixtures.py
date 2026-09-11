"""Fixed generic tasks. Reference solutions are used only by offline smoke runs."""

TASKS = {
    "small-fix": {
        "prompt": "Fix ExampleProject's bounded_count: clamp signed input to the inclusive range 0..100. Preserve its public signature. Run the relevant checks.",
        "files": {
            "Cargo.toml": '[package]\nname = "example-project"\nversion = "0.1.0"\nedition = "2021"\n\n[workspace]\n',
            "src/lib.rs": "pub fn bounded_count(n: i32) -> u32 { n.min(100) as u32 }\n",
        },
        "solution": {"src/lib.rs": "pub fn bounded_count(n: i32) -> u32 { n.clamp(0, 100) as u32 }\n"},
        "probe": "for n in [i32::MIN, -100, -1, 0, 1, 99, 100, 101, i32::MAX] { assert_eq!(subject::bounded_count(n), n.clamp(0, 100) as u32); }",
    },
    "cross-crate": {
        "prompt": "Add ExampleProject's archived status across example-core and example-api. Introduce Status::Archived and parse_status(\"archived\"). The API label for archived must be \"Archived\". Keep open -> Open and unknown -> Unknown. Parsing and status ownership belong in example-core; keep the API as a thin mapping. Run the relevant checks.",
        "files": {
            "Cargo.toml": '[workspace]\nmembers = ["crates/example-core", "crates/example-api"]\nresolver = "2"\n',
            "crates/example-core/Cargo.toml": '[package]\nname = "example-core"\nversion = "0.1.0"\nedition = "2021"\n',
            "crates/example-api/Cargo.toml": '[package]\nname = "example-api"\nversion = "0.1.0"\nedition = "2021"\n[dependencies]\nexample-core = { path = "../example-core" }\n',
            "crates/example-core/src/lib.rs": '#[derive(Debug, PartialEq)]\npub enum Status { Open }\npub fn parse_status(s: &str) -> Option<Status> { match s { "open" => Some(Status::Open), _ => None } }\n',
            "crates/example-api/src/lib.rs": 'pub fn label(s: &str) -> &\'static str { match example_core::parse_status(s) { Some(example_core::Status::Open) => "Open", None => "Unknown" } }\n',
        },
        "solution": {
            "crates/example-core/src/lib.rs": '#[derive(Debug, PartialEq)]\npub enum Status { Open, Archived }\npub fn parse_status(s: &str) -> Option<Status> { match s { "open" => Some(Status::Open), "archived" => Some(Status::Archived), _ => None } }\n',
            "crates/example-api/src/lib.rs": 'pub fn label(s: &str) -> &\'static str { match example_core::parse_status(s) { Some(example_core::Status::Open) => "Open", Some(example_core::Status::Archived) => "Archived", None => "Unknown" } }\n',
        },
        "probe": 'assert_eq!(example_core::parse_status("archived"), Some(example_core::Status::Archived)); assert_eq!(example_core::parse_status("open"), Some(example_core::Status::Open)); for s in ["", "ARCHIVED", "other"] { assert_eq!(example_core::parse_status(s), None); assert_eq!(subject::label(s), "Unknown"); } assert_eq!(subject::label("archived"), "Archived"); assert_eq!(subject::label("open"), "Open");',
    },
    "migration": {
        "prompt": "ExampleProject has already applied migrations/001.sql in production. Add migrations/002.sql to give existing and future entries an enabled INTEGER NOT NULL column with default 1. Preserve IDs, names, and the original migration bytes. The old INSERT(name) writer must still work. Use SQLite-compatible SQL and run the relevant checks.",
        "files": {"migrations/001.sql": "CREATE TABLE entries (id INTEGER PRIMARY KEY, name TEXT NOT NULL);\n"},
        "solution": {"migrations/002.sql": "ALTER TABLE entries ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1;\n"},
    },
    "frontend": {
        "prompt": "Fix ExampleProject's web/list.mjs renderList(items). Empty input must render <p role=\"status\">No entries</p>. Nonempty input must render <ul> containing one <li> per item in order. Treat item strings as text: encode &, <, >, double quotes, and apostrophes as &amp;, &lt;, &gt;, &quot;, and &#39; respectively. Preserve the named export and do not add dependencies. Run the relevant checks.",
        "files": {
            "package.json": '{"name":"example-project","private":true,"type":"module"}\n',
            "web/list.mjs": 'export function renderList(items) { return "<ul>" + items.map(x => "<li>" + x + "</li>").join("") + "</ul>"; }\n',
        },
        "solution": {"web/list.mjs": 'export function renderList(items) {\n  const escape = s => s.replace(/[&<>"\']/g, c => ({"&":"&amp;", "<":"&lt;", ">":"&gt;", \'"\':"&quot;", "\'":"&#39;"}[c]));\n  return items.length ? "<ul>" + items.map(x => "<li>" + escape(x) + "</li>").join("") + "</ul>" : \'<p role="status">No entries</p>\';\n}\n'},
    },
    "resume": {
        "prompt": "Continue the interrupted ExampleProject export task from CHECKPOINT.md and the existing worktree. Migration 002 and the export mutation already ran. Finish export.py's summary() so it reads state/exports.jsonl and returns {\"count\": number of records, \"total\": sum of amount}. Preserve the existing export records, operation receipt, migration bytes, and the completed uncommitted parser.py edit. Do not replay export mutations or create another export record. Run the relevant checks.",
        "files": {
            "CHECKPOINT.md": "Migration 002 and export operation example-export-001 completed. parser.py was updated but not committed before interruption. Next: implement summary() in export.py. Do not rerun the export.\n",
            "migrations/002.sql": "ALTER TABLE exports ADD COLUMN amount INTEGER NOT NULL DEFAULT 0;\n",
            "state/exports.jsonl": '{"id":"example-export-001","amount":7}\n',
            "state/operations.jsonl": '{"operation":"example-export-001","status":"completed"}\n',
            "parser.py": "def amount(record):\n    return 0\n",
            "export.py": "def summary():\n    return {}\n",
        },
        "interrupted_edits": {"parser.py": "def amount(record):\n    return int(record['amount'])\n"},
        "solution": {"export.py": 'import json\nfrom pathlib import Path\nfrom parser import amount\n\ndef summary():\n    records = [json.loads(line) for line in Path("state/exports.jsonl").read_text().splitlines() if line]\n    return {"count": len(records), "total": sum(amount(record) for record in records)}\n'},
    },
}


def starting_files(task):
    return {**task["files"], **task.get("interrupted_edits", {})}
