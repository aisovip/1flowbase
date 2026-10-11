const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const { buildAggregateReport, runQualityGateAggregate } = require('../core.js');
const repoRoot = path.resolve(__dirname, '../../../..');
const candidate = 'a'.repeat(40);
const artifact = (commit) => ({
  artifactName: 'test-governance-repo-tooling', scope: 'repo-tooling',
  report: { commit, status: 'passed', exitCode: 0 },
});
function report(artifacts, expected = candidate) {
  return buildAggregateReport({ repoRoot, reportType: 'ci', componentArtifacts: artifacts,
    timestamp: '2026-10-02', env: { GITHUB_SHA: expected } }).json;
}

test('aggregate accepts passing components only from the frozen candidate', () => {
  const result = report([artifact(candidate)]);
  assert.equal(result.status, 'passed');
  assert.equal(result.exitCode, 0);
  assert.equal(result.components[0].commit, candidate);
});

test('aggregate rejects mixed, missing, and unbound candidate evidence', () => {
  for (const [artifacts, expected] of [
    [[artifact(candidate), artifact('b'.repeat(40))], candidate],
    [[artifact(undefined)], candidate],
    [[artifact(candidate)], ''],
  ]) {
    const result = report(artifacts, expected);
    assert.equal(result.status, 'failed');
    assert.equal(result.exitCode, 1);
    assert.match(result.components.find((c) => c.status === 'failed').failureExcerpt, /Candidate mismatch/u);
  }
});

test('full quality jobs execute the resolver snapshot and preserve standalone scopes', () => {
  const workflow = fs.readFileSync(path.join(repoRoot, '.github/workflows/quality-gate.yml'), 'utf8');
  for (const job of ['repo-tooling-gate', 'repo-frontend-gate', 'repo-frontend-react-doctor-gate',
    'repo-backend-gate', 'backend-consistency-gate', 'coverage-frontend-gate',
    'coverage-backend-gate', 'coverage-backend-api-server-sharded',
    'coverage-backend-api-server-sharded-merge', 'container-images-gate',
    'ai-gateway-protocol-conformance', 'aggregate']) {
    const block = workflow.split(`  ${job}:\n`)[1]?.split(/\n  [a-z][a-z0-9-]*:\n/u)[0];
    assert.ok(block, `missing job: ${job}`);
    assert.match(block, /needs:(?: resolve-quality-gate-target|[\s\S]*?- resolve-quality-gate-target)/u, job);
    assert.match(block, /(?:ref|target_ref): \$\{\{ needs\.resolve-quality-gate-target\.outputs\.target_sha \}\}/u, job);
  }
  const standalone = workflow.split('  single-scope-gate:\n')[1].split('  plugin-composition-2007:')[0];
  assert.match(standalone, /ref: \$\{\{ env\.QUALITY_GATE_TARGET_BRANCH \}\}/u);
});


test('upstream macro lint policy pins only Clippy while retaining project warnings as errors', () => {
  const workflow = fs.readFileSync(path.join(repoRoot, '.github/workflows/quality-gate.yml'), 'utf8');
  assert.match(workflow, /startsWith\(matrix\.scope, 'repo-backend-clippy-'\) && '1\.98\.1' \|\| 'stable'/u);
  assert.match(workflow, /startsWith\(inputs\.scope, 'repo-backend-clippy-'\) && '1\.98\.1' \|\| 'stable'/u);
  const { buildCommands } = require('../../verify-backend.js');
  const clippy = buildCommands({ cargoJobs: 2, cargoTestThreads: 2, repoRoot, env: {}, target: 'clippy' });
  assert.ok(clippy.every((command) => command.args.slice(-2).join(' ') === '-D warnings'));
});

test('aggregate archives cannot replace current required component evidence', async (t) => {
  const cases = [
    { name: 'old failure cannot block a current pass', current: 'passed', archived: 'failed', expected: 'passed' },
    { name: 'old pass cannot hide a current failure', current: 'failed', archived: 'passed', expected: 'failed' },
    { name: 'old pass cannot satisfy a missing component', current: null, archived: 'passed', expected: 'failed' },
  ];
  for (const scenario of cases) {
    await t.test(scenario.name, async () => {
      const temporaryRoot = fs.mkdtempSync(path.join(require('node:os').tmpdir(), 'quality-aggregate-archive-'));
      try {
        const artifactRoot = path.join(temporaryRoot, 'receipts');
        const writeReceipt = (directory, scope, status) => {
          fs.mkdirSync(directory, { recursive: true });
          fs.writeFileSync(path.join(directory, 'quality-gate-report.json'), JSON.stringify({
            scope, commit: candidate, status, exitCode: status === 'passed' ? 0 : 1,
          }));
          fs.writeFileSync(path.join(directory, 'quality-gate.latest.log'), `${scope} ${status}\n`);
        };
        const archive = path.join(artifactRoot, 'test-governance-artifacts');
        writeReceipt(archive, 'ci', scenario.archived);
        writeReceipt(path.join(archive, 'parallel', 'test-governance-repo-tooling'), 'repo-tooling', scenario.archived);
        if (scenario.current) {
          // Real component artifacts may contain receipts below their own artifact root.
          writeReceipt(path.join(artifactRoot, 'test-governance-repo-tooling', 'receipt'), 'repo-tooling', scenario.current);
        }
        const result = await runQualityGateAggregate({
          repoRoot: temporaryRoot, artifactRoot, expectedScopes: ['repo-tooling'],
          reportType: 'ci', env: { GITHUB_SHA: candidate },
        });
        assert.equal(result.status, scenario.expected);
        const output = JSON.parse(fs.readFileSync(path.join(temporaryRoot, 'tmp/test-governance/quality-gate-report.json'), 'utf8'));
        assert.equal(output.components.length, 1);
        assert.equal(output.components[0].scope, 'repo-tooling');
        assert.equal(output.components[0].status, scenario.expected);
        assert.notEqual(output.components[0].artifactName, 'test-governance-artifacts');
      } finally {
        fs.rmSync(temporaryRoot, { recursive: true, force: true });
      }
    });
  }
});
