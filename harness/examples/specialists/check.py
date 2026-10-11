"""Check actual runtime metadata and unchanged synthetic fixture bytes."""
import json
import pathlib
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
          "croaks returned; fixture unchanged. Reports remain uncertified.")
