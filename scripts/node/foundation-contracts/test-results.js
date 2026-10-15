const { stripVTControlCharacters } = require('node:util');
const { parseCargoTestCounts } = require('../verify/cargo-test-results.js');

function parseExecutedTestCounts(runner, output) {
  const plain = stripVTControlCharacters(output);
  if (runner === 'cargo') return parseCargoTestCounts(plain);
  const counts = { passedCount: null, failedCount: null };
  if (runner === 'vitest') {
    for (const match of plain.matchAll(/^\s*Tests\s+([^\r\n]+)/gmu)) {
      const summary = match[1];
      counts.passedCount = (counts.passedCount ?? 0) + Number(summary.match(/(\d+) passed/u)?.[1] ?? 0);
      counts.failedCount = (counts.failedCount ?? 0) + Number(summary.match(/(\d+) failed/u)?.[1] ?? 0);
    }
  } else if (runner === 'node') {
    for (const [key, label] of [['passedCount', 'pass'], ['failedCount', 'fail']]) {
      for (const match of plain.matchAll(new RegExp(`^(?:#|ℹ)\\s*${label}\\s+(\\d+)\\s*$`, 'gmu'))) {
        counts[key] = (counts[key] ?? 0) + Number(match[1]);
      }
    }
  }
  return counts;
}

function testExecutionError({ passedCount, failedCount }) {
  if (![passedCount, failedCount].every((count) => Number.isInteger(count) && count >= 0)) {
    return 'missing or invalid executed-test summary';
  }
  if (failedCount > 0) return `test execution failed: ${passedCount} passed; ${failedCount} failed`;
  if (passedCount === 0) return 'no executed passing tests (zero execution)';
  return '';
}

module.exports = { parseExecutedTestCounts, testExecutionError };
