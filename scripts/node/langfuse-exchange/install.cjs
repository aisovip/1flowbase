'use strict';
// Installs only an explicit, locally built acceptance package through normal host APIs.
const fs = require('node:fs');
const path = require('node:path');
const assert = require('node:assert/strict');
const { loadRootCredentials, openTemporaryOwnerSession } = require('../page-debug/auth');
async function install({ apiBaseUrl, packageFile, credentialsRoot, manifest, outDir }) {
  assert.ok(!['7600', '7800'].includes(new URL(apiBaseUrl).port), 'use an independent candidate');
  const owner = await openTemporaryOwnerSession({ apiBaseUrl, ...loadRootCredentials({ repoRoot: credentialsRoot }) });
  const requests = [];
  async function request(method, route, data) {
    const form = data instanceof FormData;
    const response = await fetch(apiBaseUrl + route, { method, redirect:'error', signal:AbortSignal.timeout(120000), headers:{cookie:owner.cookie,'x-csrf-token':owner.csrfToken,...(data && !form ? {'content-type':'application/json'} : {})}, ...(data ? {body:form ? data : JSON.stringify(data)} : {}) });
    const payload = await response.json();
    requests.push({ method, route, status:response.status });
    return { response, payload };
  }
  const checked = async (...args) => { const {response,payload} = await request(...args); assert.ok(response.ok, `installation HTTP ${response.status}: ${payload.code || payload.error?.code || 'unknown'}`); return payload.data; };
  try {
    const root = '/api/console/settings/extension-center';
    const form = new FormData(); form.append('file',new Blob([fs.readFileSync(packageFile)]),path.basename(packageFile));
    let result = await request('POST',root+'/install-upload',form);
    if (result.response.status === 409 && result.payload.code === 'extension_risk_confirmation_required') {
      const warnings = result.payload.risk_challenge.warnings.map(w => w.code);
      assert.ok(warnings.every(code => ['checksum_missing','signature_missing','unsigned_package'].includes(code)), 'unexpected package risk');
      form.append('risk_override',JSON.stringify({reason:'User-authorized public Langfuse plugin acceptance from exact-source CI artifact on isolated candidate.',acknowledged_warnings:warnings}));
      result = await request('POST',root+'/install-upload',form);
    }
    assert.ok(result.response.ok, `upload HTTP ${result.response.status}: ${result.payload.code}`);
    const installationId = result.payload.data.installation.id;
    const base = `${root}/installed/${installationId}`;
    const contracts = {'service.execute':'managed-service','credential.manage':'plugin-credential','application.logs.read':'application-logs','application.logs.import':'application-logs','network.http.request':'http-egress','plugin_data.owned.write':'plugin-data'};
    for (const contribution of manifest.managed.module.contributions) {
      for (const permission of contribution.required_permissions) {
        assert.ok(contracts[permission], `unknown permission ${permission}`);
        await checked('POST',base+'/contribution-authorizations',{contribution_id:contribution.contribution_id,permission,permission_contract_id:contracts[permission],permission_contract_version:'1',resource_scope:permission === 'plugin_data.owned.write' ? {kind:'owned_collection',collection_code:'connection'} : {kind:'system'}});
      }
    }
    await checked('POST',base+'/enable',{});
    const receipt = { installation_id:installationId, requests };
    fs.mkdirSync(outDir,{recursive:true}); fs.writeFileSync(path.join(outDir,'installation.json'),JSON.stringify(receipt,null,2));
    return receipt;
  } finally { await owner.dispose(); }
}
module.exports = { install };
