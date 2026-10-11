"""Check actual runtime metadata and unchanged synthetic fixture bytes."""
import json
import pathlib
import re
import sys


def check(mode, fixture, source, output=None):
    output = output or fixture
    events = [json.loads(line) for line in (output / "events.jsonl").read_text().splitlines()]
    result = json.loads((output / "result.json").read_text())
    assert result["status"] == "completed", "run did not complete"
    assert result["usage_complete"] is True, "received-response usage is incomplete"
    expected = {"scout": ("repository_scout", 1), "analyst": ("test_analyst", 2)}
    if mode != "both":
        expected = {mode: expected[mode]}
    croaks = [event for event in events if event["event"] == "croak"]
    assert {event["id"] for event in croaks} == set(expected), "unexpected workers"
    assert all(event["status"] == "returned" for event in croaks), "blocked croak"
    reports = result["tadpole_croaks"]
    assert len(reports) == len(expected), "missing or extra structured croaks"
    assert {report["id"] for report in reports} == set(expected), "wrong structured worker IDs"
    required_paths = {
        "scout": {"README.md", "src/greeting.rs", "tests/greeting.rs"},
        "analyst": {"tests/greeting.rs"},
    }
    required_ids = {
        "scout": {"readme", "greeting_source", "greeting_test"},
        "analyst": {"inline_formatting_not_greeting_call"},
    }
    for report in reports:
        worker = report["id"]
        assert report["status"] == "returned", "structured croak is blocked"
        assert report["contract"]["baseline_complete"] is True, "incomplete baseline"
        assert report["contract"]["citations_verified"] is True, "citations unverified"
        assert report["contract"]["citation_subjects_required"] == len(required_paths[worker])
        assert report["contract"]["citation_subjects_complete"] is True, "required subjects incomplete"
        payload = report["findings"]
        assert payload["citations_verified"] is True and payload["claims_certified"] is False
        items = payload["findings"]
        assert len({finding["id"] for finding in items}) == len(items), "duplicate finding IDs"
        assert {finding["id"] for finding in items} == required_ids[worker], "wrong declared subject IDs"
        findings = {finding["path"]: finding for finding in items}
        assert len(findings) == len(items), "duplicate fixture citation paths"
        assert set(findings) == required_paths[worker], "missing or unexpected fixture findings"
        for path, finding in findings.items():
            lines = (source / path).read_text().splitlines()
            assert finding["kind"] == "observation" and finding["statement"].strip()
            assert finding["path"] == path, "wrong fixture citation path"
            assert finding["start_line"] == 1 and finding["end_line"] == len(lines)
            assert finding["excerpt"] == "\n".join(lines), "wrong fixture excerpt"
        if worker == "analyst":
            text = findings["tests/greeting.rs"]["excerpt"]
            assert 'assert_eq!(format!("Hello, {}!", "pond"), "Hello, pond!");' in text
            assert re.search(r"\bgreeting\s*\(", text) is None, \
                "fixture contains a direct greeting call"
    for worker, (specialty, count) in expected.items():
        child = [event["detail"] for event in events
                 if event["event"] == "tadpole" and event["id"] == worker]
        assert any(event["event"] == "contract_started" and event["specialty"] == specialty
                   for event in child), f"{worker}: contract did not start"
        reasoning = next(index for index, event in enumerate(child)
                         if event["event"] == "reasoning_started")
        baseline = child[:reasoning]
        tools = [event for event in baseline if event["event"] == "tool_finished"]
        assert len(tools) == count and all(not event["is_error"] for event in tools), \
            f"{worker}: baseline tools did not succeed before reasoning"
        required = {"contract_inventory": "list_files"}
        if worker == "analyst":
            required["contract_test_signals"] = "search_files"
        assert {event["step"]: event["tool"] for event in tools} == required, \
            f"{worker}: wrong baseline steps"
        decisions = [event for event in baseline
                     if event["event"] == "laya_decision" and event["stage"] == "evidence"]
        assert len(decisions) == count and all(
            event["result"]["decision"] == "continue_planned_step"
            and event["result"]["step_id"] == event["step"] for event in decisions
        ), f"{worker}: baseline evidence was not accepted by Laya"
    tools = [event.get("detail", event) for event in events]
    assert not any(event.get("tool") in {"write_file", "apply_tadpole_patch", "propose_patch"}
                   for event in tools), "unexpected mutation or proposal"
    files = {path.relative_to(fixture) for path in fixture.rglob("*") if path.is_file()}
    originals = {path.relative_to(source) for path in source.rglob("*") if path.is_file()}
    logs = {pathlib.Path("result.json"), pathlib.Path("events.jsonl")} if output == fixture else set()
    assert files == originals | logs, \
        "fixture file set changed"
    for path in originals:
        assert (fixture / path).read_bytes() == (source / path).read_bytes(), \
            f"fixture contents changed: {path}"


if __name__ == "__main__":
    try:
        check(sys.argv[1], pathlib.Path(sys.argv[2]), pathlib.Path(sys.argv[3]),
              pathlib.Path(sys.argv[4]) if len(sys.argv) > 4 else None)
    except (AssertionError, KeyError, ValueError, StopIteration, OSError) as error:
        print(f"FAIL: {error or 'required reasoning event missing'}", file=sys.stderr)
        sys.exit(1)
    print("PASS: specialist baselines and Laya evidence precede reasoning; "
          "expected fixture findings have exact citations; fixture unchanged. "
          "Model interpretations remain uncertified.")
