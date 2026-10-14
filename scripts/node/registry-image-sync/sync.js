#!/usr/bin/env node
const fs = require('node:fs');
const path = require('node:path');
const os = require('node:os');
const crypto = require('node:crypto');
const { spawnSync } = require('node:child_process');

const COMPONENTS = [
  { job: 'publish-api-server', image: '1flowbase-api-server', prefix: 'api', manifest: 'api/apps/api-server/Cargo.toml' },
  { job: 'publish-web', image: '1flowbase-web', prefix: 'web', manifest: 'web/app/package.json' },
];

function validateConfig(env) {
  for (const name of ['GITHUB_REPOSITORY', 'GH_TOKEN', 'SOURCE_RUN_ID', 'ACR_REGISTRY', 'ACR_REPOSITORY', 'ACR_USERNAME', 'ACR_PASSWORD']) {
    if (!env[name]?.trim()) throw new Error(`Missing ${name}`);
  }
  if (!/^\d+$/.test(env.SOURCE_RUN_ID)) throw new Error('SOURCE_RUN_ID must be numeric');
  if (!/^[a-zA-Z0-9.-]+(?::\d+)?$/.test(env.ACR_REGISTRY)) throw new Error('ACR_REGISTRY must be a registry hostname');
  if (!/^[a-z0-9]+(?:[._-][a-z0-9]+)*(?:\/[a-z0-9]+(?:[._-][a-z0-9]+)*)+$/.test(env.ACR_REPOSITORY)) {
    throw new Error('ACR_REPOSITORY must be a namespace/repository path');
  }
}

function validateRun(run, repository) {
  if (run.repository?.full_name !== repository || run.head_repository?.full_name !== repository ||
      run.path !== '.github/workflows/container-images.yml' || run.head_branch !== 'latest' ||
      run.status !== 'completed' || run.conclusion !== 'success' ||
      !['push', 'workflow_dispatch'].includes(run.event) || !/^[a-f0-9]{40}$/.test(run.head_sha)) {
    throw new Error('Source must be a successful container-images.yml run from this repository on latest');
  }
}

function imageTag(component, source) {
  const version = component.prefix === 'web' ? JSON.parse(source).version :
    source.match(/^version\s*=\s*"([^"]+)"/m)?.[1];
  if (typeof version !== 'string' || !/^\d+\.\d+\.\d+(?:-[a-zA-Z0-9.-]+)?$/.test(version)) {
    throw new Error(`Invalid ${component.prefix} version`);
  }
  return `v${version}`;
}

function digest(raw) {
  return `sha256:${crypto.createHash('sha256').update(raw).digest('hex')}`;
}

function runtimeManifests(raw) {
  const index = JSON.parse(raw);
  return ['amd64', 'arm64'].map(architecture => {
    const matches = (index.manifests || []).filter(item => item.platform?.os === 'linux' && item.platform.architecture === architecture);
    if (matches.length !== 1 || !/^sha256:[a-f0-9]{64}$/.test(matches[0].digest)) {
      throw new Error(`Expected exactly one linux/${architecture} runtime manifest`);
    }
    return { platform: { os: 'linux', architecture,
      ...(matches[0].platform.variant ? { variant: matches[0].platform.variant } : {}) }, digest: matches[0].digest };
  });
}

function regctlAdapter(authFile, execute = spawnSync, executable = 'regctl') {
  return (args, input) => {
    const result = execute(executable, args, {
      input, encoding: 'utf8', timeout: 30 * 60 * 1000, maxBuffer: 8 * 1024 * 1024,
      env: { ...process.env, REGCTL_CONFIG: authFile },
    });
    if (result.error || result.status !== 0) {
      let detail = args[0] === 'registry' && args[1] === 'login' ? '' :
        (result.stderr || result.error?.message || '').trim().split('\n').at(-1);
      for (const secret of [process.env.ACR_PASSWORD, process.env.GH_TOKEN, input?.trim()].filter(Boolean)) {
        detail = detail.replaceAll(secret, '[redacted]');
      }
      detail = detail.replace(/https?:\/\/[^\s]+/g, '[registry URL]').slice(0, 600);
      throw new Error(`regctl ${args.slice(0, 2).join(' ')} failed (exit ${result.status ?? 'unknown'})${detail ? `: ${detail}` : ''}`);
    }
    return result.stdout;
  };
}

async function githubApi(env, endpoint) {
  const response = await fetch(`https://api.github.com/repos/${env.GITHUB_REPOSITORY}/${endpoint}`, {
    headers: { Authorization: `Bearer ${env.GH_TOKEN}`, Accept: 'application/vnd.github+json', 'X-GitHub-Api-Version': '2022-11-28' },
    signal: AbortSignal.timeout(30000),
  });
  if (!response.ok) throw new Error(`GitHub API ${endpoint.split('?')[0]} returned ${response.status}`);
  return response.json();
}

async function syncImages(env, { api, regctl, report = { images: [] }, log = console.log }) {
  validateConfig(env);
  const run = await api(`actions/runs/${env.SOURCE_RUN_ID}`);
  validateRun(run, env.GITHUB_REPOSITORY);
  report.source_run_id = run.id;
  report.source_sha = run.head_sha;
  report.destination = `${env.ACR_REGISTRY}/${env.ACR_REPOSITORY}`;
  const jobs = [];
  for (let page = 1; ; page++) {
    const result = await api(`actions/runs/${run.id}/jobs?filter=latest&per_page=100&page=${page}`);
    jobs.push(...result.jobs);
    if (result.jobs.length < 100) break;
  }
  const components = COMPONENTS.filter(c => jobs.some(j => j.name === c.job && j.conclusion === 'success'));
  if (!components.length) throw new Error('Source run did not publish any component');

  regctl(['registry', 'login', env.ACR_REGISTRY, '--user', env.ACR_USERNAME, '--pass-stdin'], `${env.ACR_PASSWORD}\n`);
  regctl(['registry', 'login', 'ghcr.io', '--user', env.GITHUB_ACTOR || env.GITHUB_REPOSITORY.split('/')[0], '--pass-stdin'], `${env.GH_TOKEN}\n`);
  for (const component of components) {
    const file = await api(`contents/${component.manifest}?ref=${run.head_sha}`);
    const tag = imageTag(component, Buffer.from(file.content, 'base64').toString('utf8'));
    const sourceRepo = `ghcr.io/${env.GITHUB_REPOSITORY.split('/')[0].toLowerCase()}/${component.image}`;
    const source = `${sourceRepo}:${tag}`;
    const sourceRaw = regctl(['manifest', 'get', source, '--format', 'raw-body']);
    const sourceDigest = digest(sourceRaw);
    const platforms = runtimeManifests(sourceRaw);
    const pinnedSource = `${sourceRepo}@${sourceDigest}`;
    const labels = JSON.parse(regctl(['image', 'inspect', pinnedSource, '--platform', 'linux/amd64', '--format', '{{json .Config.Labels}}']));
    if (labels?.['org.opencontainers.image.revision'] !== run.head_sha ||
        labels?.['org.opencontainers.image.version'] !== tag) {
      throw new Error(`${component.prefix} official image does not match source run; candidate-only builds cannot be mirrored`);
    }
    const entry = { component: component.prefix, source: pinnedSource, tag, destination_tag: `${component.prefix}-${tag}`, platforms,
      excluded_manifests: JSON.parse(sourceRaw).manifests.length - platforms.length, latest_updated: false };
    report.images.push(entry);
    function copyAndVerify(destinationTag) {
      const target = `${report.destination}:${destinationTag}`;
      log(`Copy ${pinnedSource} -> ${target}`);
      // Personal ACR rejects OCI empty-config attestation manifests. Keep both runtime
      // architectures, preserving each image digest while rebuilding only the index.
      regctl(['index', 'create', target, '--ref', pinnedSource, '--platform', 'linux/amd64', '--platform', 'linux/arm64']);
      const destinationRaw = regctl(['manifest', 'get', target, '--format', 'raw-body']);
      if (JSON.parse(destinationRaw).manifests?.length !== 2 ||
          JSON.stringify(runtimeManifests(destinationRaw)) !== JSON.stringify(platforms)) {
        throw new Error(`${destinationTag} remote platform digest mismatch`);
      }
      return digest(destinationRaw);
    }
    entry.verified_digest = copyAndVerify(entry.destination_tag);
    // A historical retry must never replace a newer official latest image.
    if (digest(regctl(['manifest', 'get', `${sourceRepo}:latest`, '--format', 'raw-body'])) === sourceDigest) {
      if (copyAndVerify(`${component.prefix}-latest`) !== entry.verified_digest) {
        throw new Error(`${component.prefix}-latest differs from the verified version tag`);
      }
      entry.latest_updated = true;
    } else {
      log(`Keep ${component.prefix}-latest: GHCR latest has moved to another image`);
    }
  }
  return report;
}

async function main(env = process.env) {
  const report = { ok: false, images: [] };
  const output = path.resolve('tmp/test-governance/aliyun-image-sync/report.json');
  const authDirectory = fs.mkdtempSync(path.join(env.RUNNER_TEMP || os.tmpdir(), 'registry-image-sync-'));
  fs.chmodSync(authDirectory, 0o700);
  try {
    await syncImages(env, {
      api: endpoint => githubApi(env, endpoint),
      regctl: regctlAdapter(path.join(authDirectory, 'config.json'), spawnSync, env.REGCTL_EXECUTABLE),
      report,
    });
    report.ok = true;
  } catch (error) {
    report.error = error.message;
    throw error;
  } finally {
    fs.rmSync(authDirectory, { recursive: true, force: true });
    fs.mkdirSync(path.dirname(output), { recursive: true });
    fs.writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`);
  }
}

module.exports = { COMPONENTS, validateConfig, validateRun, imageTag, digest, runtimeManifests, regctlAdapter, syncImages, main };
if (require.main === module) main().catch(error => { console.error(error.message); process.exitCode = 1; });
