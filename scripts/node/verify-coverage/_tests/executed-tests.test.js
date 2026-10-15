const test = require('node:test');
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { main, buildBackendCommands } = require('../../verify-coverage.js');

// Exercise the real coverage orchestration. A passing neighboring suite and a
// perfect coverage summary must not mask one empty or failed test selection.
test('each control-plane coverage command requires its own executed passing tests', async () => {
  const labels = buildBackendCommands({
    repoRoot: '/fixture', cargoParallelism: 2, cargoTestThreads: 2,
    backendKeys: ['control-plane'],
  }).map(command => command.label);
  for (const label of labels) {
    for (const [output, expected] of [
      ['', 1],
      ['test result: ok. 0 passed; 0 failed; 15 ignored;', 1],
      ['test result: FAILED. 1 passed; 1 failed;', 1],
      ['test result: ok. 1 passed; 0 failed;', 0],
      ['test result: \u001b[32mok\u001b[0m. 2 passed; 0 failed;', 0],
    ]) {
      const repoRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'coverage-execution-'));
      try {
        const commands = buildBackendCommands({
          repoRoot, cargoParallelism: 2, cargoTestThreads: 2,
          backendKeys: ['control-plane'],
        });
        let diagnostics = '';
        let reportsRead = 0;
        const status = await main(['backend', 'control-plane'], {
          repoRoot, env: {},
          runtimeConfig: { backend: { cargoJobs: 2, cargoTestThreads: 2 } },
          preflightSpawnSyncImpl: () => ({ status: 0 }),
          writeStdout() {}, writeStderr(text) { diagnostics += text; },
          managedRunnerImpl(options) {
            return options.runCommandSequenceImpl({
              ...options,
              spawnSyncImpl(_command, args) {
                const selected = commands.find(command =>
                  JSON.stringify(command.args) === JSON.stringify(args));
                return {
                  status: 0,
                  stdout: args[1] === 'clean'
                    ? ''
                    : selected?.label === label ? output : 'test result: ok. 3 passed; 0 failed;',
                  stderr: '',
                };
              },
            });
          },
          readFileSyncImpl() {
            reportsRead += 1;
            return JSON.stringify({ data: [{ totals: { lines: { percent: 100 } } }] });
          },
        });
        assert.equal(status, expected, `${label}: ${output}`);
        assert.equal(reportsRead > 0, expected === 0);
        if (expected) assert.ok(diagnostics.includes(`${label}: no executed passing tests`));
      } finally {
        fs.rmSync(repoRoot, { recursive: true, force: true });
      }
    }
  }
});
