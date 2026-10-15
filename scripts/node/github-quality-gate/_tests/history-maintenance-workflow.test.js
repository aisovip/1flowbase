const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");

// Feature-gated binaries are absent from the ordinary workspace Clippy lane.
// Keep their lint evidence attached to the existing opt-in maintenance job.
test("history maintenance compiles all owned feature targets under strict Clippy", () => {
  const workflow = fs.readFileSync(
    path.resolve(__dirname, "../../../../.github/workflows/gateway-target-handoff.yml"),
    "utf8",
  );
  const job = workflow.slice(workflow.indexOf("  history-fixtures:"));
  assert.match(job, /uses: dtolnay\/rust-toolchain@stable\n\s+with:\n\s+components: clippy/u);
  const step = job.match(/      - name: Lint maintenance feature targets\n[\s\S]*?(?=      - (?:name:|uses:))/u)?.[0];
  assert.ok(step, "the maintenance lint step must exist in history-fixtures");
  assert.match(step, /if: \$\{\{ !cancelled\(\) && inputs\.build_history_maintenance \}\}/u);
  assert.match(step, /cargo clippy --locked --manifest-path api\/Cargo\.toml/u);
  for (const pkg of ["api-server", "control-plane", "control-plane-contracts", "storage-durable-postgres", "control-plane-postgres-tests"]) {
    assert.ok(step.includes(`-p ${pkg} `), `missing package: ${pkg}`);
  }
  assert.match(step, /--all-targets/u);
  assert.match(step, /--features api-server\/responses-history-migration,control-plane-postgres-tests\/responses-history-migration/u);
  assert.match(step, /-- -D warnings/u);
  assert.doesNotMatch(step, /--(?:workspace|all-features)/u);
  assert.match(step, /history-clippy\.log; exit 1;/u);
  assert.match(job, /if: always\(\)[\s\S]*path: tmp\/test-governance\/gateway-target-handoff\/ci/u);
});
