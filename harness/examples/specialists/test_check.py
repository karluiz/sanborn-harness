import json
import pathlib
import tempfile
import unittest

from check import check


class CheckExamples(unittest.TestCase):
    def test_success_and_false_positive_rejections(self):
        source = pathlib.Path(__file__).parent / "fixture"
        for mode in ("scout", "analyst", "both"):
            with tempfile.TemporaryDirectory() as directory:
                fixture = pathlib.Path(directory)
                for path in source.rglob("*"):
                    if path.is_file():
                        target = fixture / path.relative_to(source)
                        target.parent.mkdir(parents=True, exist_ok=True)
                        target.write_bytes(path.read_bytes())
                events = []
                for worker in (("scout", "analyst") if mode == "both" else (mode,)):
                    specialty = "repository_scout" if worker == "scout" else "test_analyst"
                    details = [{"event": "contract_started", "specialty": specialty}]
                    steps = [("contract_inventory", "list_files")]
                    if worker == "analyst":
                        steps.append(("contract_test_signals", "search_files"))
                    for step, tool in steps:
                        details.extend([
                            {"event": "tool_finished", "step": step, "tool": tool, "is_error": False},
                            {"event": "laya_decision", "stage": "evidence", "step": step,
                             "result": {"decision": "continue_planned_step", "step_id": step}},
                        ])
                    details.append({"event": "reasoning_started"})
                    events.extend({"event": "tadpole", "id": worker, "detail": detail}
                                  for detail in details)
                    events.append({"event": "croak", "id": worker, "status": "returned"})
                result = {"status": "completed", "usage_complete": True}
                (fixture / "result.json").write_text(json.dumps(result))

                def write_events(items):
                    (fixture / "events.jsonl").write_text(
                        "\n".join(json.dumps(item) for item in items))

                write_events(events)
                check(mode, fixture, source)
                with tempfile.TemporaryDirectory() as logs:
                    output = pathlib.Path(logs)
                    for filename in ("events.jsonl", "result.json"):
                        (output / filename).write_bytes((fixture / filename).read_bytes())
                        (fixture / filename).unlink()
                    check(mode, fixture, source, output)
                    for filename in ("events.jsonl", "result.json"):
                        (fixture / filename).write_bytes((output / filename).read_bytes())
                variants = [
                    [event for event in events if event["event"] != "croak"],
                    [dict(event, status="blocked") if event["event"] == "croak" else event
                     for event in events],
                    [event for event in events if event.get("detail", {}).get("event")
                     != "laya_decision"],
                    [{"event": "tadpole", "id": next(event["id"] for event in events),
                      "detail": {"event": "reasoning_started"}}] + events,
                    events + [{"event": "tool_finished", "tool": "write_file"}],
                ]
                for variant in variants:
                    write_events(variant)
                    with self.assertRaises(AssertionError):
                        check(mode, fixture, source)
                write_events(events)
                (fixture / "src/greeting.rs").write_text("unexpected edit")
                with self.assertRaises(AssertionError):
                    check(mode, fixture, source)
                (fixture / "src/greeting.rs").write_bytes((source / "src/greeting.rs").read_bytes())
                (fixture / "extra.txt").write_text("unexpected file")
                with self.assertRaises(AssertionError):
                    check(mode, fixture, source)


if __name__ == "__main__":
    unittest.main()
