const test = require('node:test');
const assert = require('node:assert/strict');
const { COMPONENTS, syncImages, validateRun, imageTag, digest, runtimeManifests, regctlAdapter } = require('../sync.js');

const sha = 'a'.repeat(40);
const env = {
  GITHUB_REPOSITORY: 'owner/project', GH_TOKEN: 'github-secret', SOURCE_RUN_ID: '123',
  ACR_REGISTRY: 'registry.example.com', ACR_REPOSITORY: 'namespace/images',
  ACR_USERNAME: 'user', ACR_PASSWORD: 'registry-secret',
};
const run = {
  id: 123, repository: { full_name: env.GITHUB_REPOSITORY }, head_repository: { full_name: env.GITHUB_REPOSITORY },
  path: '.github/workflows/container-images.yml', head_branch: 'latest', head_sha: sha,
  event: 'push', status: 'completed', conclusion: 'success',
};

function fixture({ webPublished = true, stale = false, mismatched = false, corrupt = false } = {}) {
  const copies = [], calls = [], inputs = [], logs = [];
  const raw = {}, runtimeRaw = {};
  for (const [component, hex] of [['api', ['a', 'b']], ['web', ['c', 'd']]]) {
    const manifests = ['amd64', 'arm64'].map((architecture, i) => ({ platform: { os: 'linux', architecture }, digest: `sha256:${hex[i].repeat(64)}` }));
    runtimeRaw[component] = JSON.stringify({ schemaVersion: 2, manifests });
    raw[component] = JSON.stringify({ schemaVersion: 2, manifests: [...manifests,
      { platform: { os: 'unknown', architecture: 'unknown' }, digest: `sha256:${'e'.repeat(64)}` }] });
  }
  const api = async endpoint => {
    if (endpoint === 'actions/runs/123') return run;
    if (endpoint.includes('/jobs?')) return { jobs: [
      { name: 'publish-api-server', conclusion: 'success' },
      { name: 'publish-web', conclusion: webPublished ? 'success' : 'skipped' },
    ] };
    if (endpoint.startsWith('contents/')) {
      assert.ok(endpoint.endsWith(`?ref=${sha}`));
      const text = endpoint.includes('Cargo.toml') ? '[package]\nversion = "1.2.3"\n' : '{"version":"2.3.4"}';
      return { content: Buffer.from(text).toString('base64') };
    }
    throw new Error(`Unexpected API endpoint ${endpoint}`);
  };
  const regctl = (args, input) => {
    calls.push(args); if (input) inputs.push(input);
    if (args[0] === 'registry') return '';
    const target = args[2];
    const component = target.includes('api-server') || target.includes(':api-') ? 'api' : 'web';
    if (args[0] === 'index') { copies.push(args); return ''; }
    if (args[0] === 'manifest') {
      if (target.includes('registry.example.com')) return corrupt ?
        runtimeRaw[component].replace(`sha256:${(component === 'api' ? 'a' : 'c').repeat(64)}`, `sha256:${'f'.repeat(64)}`) : runtimeRaw[component];
      if (stale && target.endsWith(':latest')) return 'newer manifest';
      return raw[component];
    }
    return JSON.stringify({
      'org.opencontainers.image.revision': mismatched ? 'b'.repeat(40) : sha,
      'org.opencontainers.image.version': component === 'api' ? 'v1.2.3' : 'v2.3.4',
    });
  };
  return { api, regctl, log: line => logs.push(line), copies, calls, inputs, logs, raw, runtimeRaw };
}

test('copies independently versioned runtime images by digest, excludes attestations and verifies both architectures', async () => {
  const f = fixture(); const result = await syncImages(env, f);
  assert.deepEqual(result.images.map(i => i.destination_tag), ['api-v1.2.3', 'web-v2.3.4']);
  assert.equal(f.copies.length, 4);
  for (const copy of f.copies) {
    assert.ok(copy.includes('linux/amd64')); assert.ok(copy.includes('linux/arm64'));
    assert.match(copy[copy.indexOf('--ref') + 1], /@sha256:[a-f0-9]{64}$/);
  }
  assert.ok(result.images.every(i => i.latest_updated && i.verified_digest === digest(f.runtimeRaw[i.component]) && i.excluded_manifests === 1));
  assert.ok(result.images.every(i => i.verified_digest !== digest(f.raw[i.component])));
  assert.deepEqual(f.inputs, ['registry-secret\n', 'github-secret\n']);
  assert.doesNotMatch(JSON.stringify(f.calls) + JSON.stringify(f.logs) + JSON.stringify(result), /registry-secret|github-secret/);
});

test('historical retry copies version tags but preserves latest when GHCR latest has changed', async () => {
  const f = fixture({ stale: true }); const result = await syncImages(env, f);
  assert.equal(f.copies.length, 2);
  assert.ok(f.copies.every(c => !c[2].endsWith('-latest')));
  assert.ok(result.images.every(i => !i.latest_updated));
});

test('only components actually published by the source run are mirrored', async () => {
  const f = fixture({ webPublished: false }); const result = await syncImages(env, f);
  assert.deepEqual(result.images.map(i => i.component), ['api']);
  assert.ok(f.calls.every(c => !c.join(' ').includes('1flowbase-web')));
});

test('rejects candidate-only or unrelated official image before copying', async () => {
  const f = fixture({ mismatched: true });
  await assert.rejects(syncImages(env, f), /does not match source run/);
  assert.equal(f.copies.length, 0);
});

test('a remote manifest mismatch fails and prevents latest promotion', async () => {
  const f = fixture({ corrupt: true }); const report = { images: [] };
  await assert.rejects(syncImages(env, { ...f, report }), /remote platform digest mismatch/);
  assert.equal(f.copies.length, 1); assert.equal(report.images[0].latest_updated, false);
});

test('rejects failed, foreign, wrong-workflow and wrong-branch source runs', () => {
  for (const change of [
    { conclusion: 'failure' }, { status: 'in_progress' }, { event: 'pull_request' },
    { head_branch: 'dev' }, { path: '.github/workflows/verify.yml' },
    { repository: { full_name: 'foreign/repo' } }, { head_repository: { full_name: 'foreign/repo' } },
  ]) assert.throws(() => validateRun({ ...run, ...change }, env.GITHUB_REPOSITORY), /Source must/);
});

test('invalid registry settings fail before API access or credential use', async () => {
  const f = fixture();
  await assert.rejects(syncImages({ ...env, ACR_REGISTRY: 'https://registry.example.com' }, f), /hostname/);
  await assert.rejects(syncImages({ ...env, ACR_REPOSITORY: '../path' }, f), /namespace/);
  assert.equal(f.calls.length, 0);
});

test('credentials travel over stdin and subprocess errors cannot expose credentials', () => {
  const execute = (command, args, options) => {
    assert.equal(command, 'regctl'); assert.equal(options.input, 'secret\n');
    assert.ok(args.includes('--pass-stdin')); assert.ok(!args.includes('secret'));
    return { status: 1, stderr: 'credential secret echoed by tool' };
  };
  assert.throws(() => regctlAdapter('/private/auth.json', execute)(['registry', 'login', 'registry', '--pass-stdin'], 'secret\n'),
    error => error.message.includes('failed') && !error.message.includes('secret'));
});

test('version input rejects malformed or absent tags', () => {
  assert.equal(imageTag(COMPONENTS[0], '[package]\nversion = "1.2.3-rc.1"'), 'v1.2.3-rc.1');
  assert.throws(() => imageTag(COMPONENTS[1], '{"version":"bad/tag"}'), /Invalid/);
});

test('copy failures retain transport diagnosis while redacting signed request URLs', () => {
  const execute = () => ({ status: 1, stderr: 'request https://registry.example/upload?signature=private-token TLS handshake timeout' });
  assert.throws(() => regctlAdapter('/private/auth.json', execute)(['index', 'create', 'destination', '--ref', 'source']),
    error => error.message.includes('TLS handshake timeout') && !error.message.includes('private-token'));
});

test('a missing or duplicate supported architecture fails instead of accepting a partial mirror', () => {
  const f = fixture();
  const partial = JSON.parse(f.runtimeRaw.api); partial.manifests.pop();
  assert.throws(() => runtimeManifests(JSON.stringify(partial)), /linux\/arm64/);
  const duplicate = JSON.parse(f.runtimeRaw.api); duplicate.manifests.push(duplicate.manifests[0]);
  assert.throws(() => runtimeManifests(JSON.stringify(duplicate)), /linux\/amd64/);
});
