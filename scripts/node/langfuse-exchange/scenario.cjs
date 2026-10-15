'use strict';
const fs = require('node:fs');
const path = require('node:path');
const crypto = require('node:crypto');
const assert = require('node:assert/strict');
const { parseArgs } = require('node:util');
const { execFileSync } = require('node:child_process');
const { loadRootCredentials, openTemporaryOwnerSession } = require('../page-debug/auth');
const { syntheticBatch } = require('../agent-logs-exchange/fixtures.cjs');
const { capture } = require('./browser.cjs');
const root = path.resolve(__dirname,'../../..');
async function main() {
  const { values } = parseArgs({options:{'api-base-url':{type:'string'},'web-base-url':{type:'string'},'credentials-root':{type:'string'},'langfuse-private-file':{type:'string'},'langfuse-storage-private-file':{type:'string'},'out-dir':{type:'string'},'native-application-id':{type:'string'},'deadline-ms':{type:'string',default:'120000'}}});
  for(const field of ['api-base-url','web-base-url','credentials-root','langfuse-private-file','out-dir']) assert.ok(values[field],`missing ${field}`);
  const args=Object.fromEntries(Object.entries(values).map(([k,v])=>[k.replace(/-([a-z])/g,(_,c)=>c.toUpperCase()),v]));
  args.deadlineMs=Number(args.deadlineMs);args.outDir=path.resolve(args.outDir);
  for(const url of [args.apiBaseUrl,args.webBaseUrl]) assert.ok(!['7600','7800'].includes(new URL(url).port),'independent candidate only');
  assert.ok(args.outDir.startsWith(path.join(root,'tmp/test-governance/')),'evidence stays within candidate governance');
  fs.mkdirSync(path.join(args.outDir,'private'),{recursive:true,mode:0o700});
  const receipt={source_sha:execFileSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8'}).trim(),result:'incomplete',checks:[],exports:[],imports:[],screenshots:[],created_application_ids:[]};
  const checkpoint=()=>fs.writeFileSync(path.join(args.outDir,'receipt.json'),JSON.stringify(receipt,null,2));
  const credentials=loadRootCredentials({repoRoot:args.credentialsRoot});
  const owner=await openTemporaryOwnerSession({apiBaseUrl:args.apiBaseUrl,...credentials});
  const keys=[];
  async function request(method,route,body,token) {
    const response=await fetch(args.apiBaseUrl+route,{method,redirect:'error',signal:AbortSignal.timeout(args.deadlineMs),headers:{'content-type':'application/json',...(token?{Authorization:`Bearer ${token}`}:{cookie:owner.cookie,'x-csrf-token':owner.csrfToken})},...(body===undefined?{}:{body:JSON.stringify(body)})});
    const result=await response.json(); assert.ok(response.ok,`HTTP ${response.status} ${route}: ${result.code||'unknown'}`);
    return route.endsWith('/otlp/export')?result:result.data;
  }
  try {
    const langfuse=JSON.parse(fs.readFileSync(args.langfusePrivateFile,'utf8'));langfuse.base_url=langfuse.base_url.replace(/\/$/,'');
    const suffix=crypto.randomUUID().slice(0,8);
    const apps=[];
    for(const label of ['Langfuse plugin source','Langfuse plugin imported']) {
      const app=await request('POST','/api/console/applications',{application_type:'agent_logs',name:`${label} ${suffix}`,description:'Public Langfuse plugin browser acceptance fixture',icon:null,icon_type:null,icon_background:null});
      apps.push(app);receipt.created_application_ids.push(app.id);
      await request('PATCH',`/api/console/applications/${app.id}/api-status`,{api_enabled:true});
      const key=await request('POST',`/api/console/applications/${app.id}/api-keys`,{name:`langfuse-${suffix}`,expires_at:null});keys.push({application_id:app.id,...key});
    }
    const batch=syntheticBatch();const ingested=await request('POST','/api/logs/v1/events',batch,keys[0].token);
    assert.equal(ingested.accepted_events,batch.events.length);
    const times=batch.events.map(e=>Date.parse(e.occurred_at));
    const sources=[{application_id:apps[0].id,name:apps[0].name,application_type:'agent_logs',started_at_from:new Date(Math.min(...times)-1000).toISOString(),started_at_to:new Date(Math.max(...times)+1000).toISOString(),expected_events:batch.events.length}];
    // Read existing native history, without model calls or creation of native execution state.
    const visible=await request('GET','/api/console/managed-services/langfuse/applications');assert.equal(visible.success,true);
    let native;
    for(const app of visible.items.filter(a=>a.application_type==='agent_flow'&&(!args.nativeApplicationId||a.application_id===args.nativeApplicationId))) {
      const rows=await request('POST','/api/console/managed-services/langfuse/records/query',{application_id:app.application_id,limit:1});assert.equal(rows.success,true);
      if(rows.items?.length){native={...app,record:rows.items[0]};break;}
    }
    assert.ok(native,'retained native AgentFlow history required');
    const at=Date.parse(native.record.started_at);assert.ok(Number.isFinite(at));
    sources.push({...native,started_at_from:new Date(at).toISOString(),started_at_to:new Date(at+1).toISOString()});
    receipt.sources=sources;checkpoint();
    const authorization='Basic '+Buffer.from(`${langfuse.public_key}:${langfuse.secret_key}`).toString('base64');
    async function waitRemote(traceIds) {
      const deadline=Date.now()+args.deadlineMs;
      while(true) {
        let ready=true;
        for(const id of traceIds) {
          const response=await fetch(`${langfuse.base_url}/api/public/traces/${encodeURIComponent(id)}`,{headers:{Authorization:authorization},signal:AbortSignal.timeout(30000),redirect:'error'});
          if(response.status===404){ready=false;continue;}
          assert.ok(response.ok,`Langfuse trace HTTP ${response.status}`);
          const trace=await response.json();if(!trace.observations?.length)ready=false;
        }
        if(ready)return;
        assert.ok(Date.now()<deadline,'Langfuse persistence deadline');await new Promise(r=>setTimeout(r,1000));
      }
    }
    async function verifyTarget(recordIds) {
      let first;let importedTasks=0;
      for(const id of recordIds) {
        const value=await request('GET',`/api/console/applications/${apps[1].id}/logs/records/${id}`);
        assert.equal(value.source_kind,'imported');assert.equal(value.native_run_id,null,'historical import must not create native runs');
        if(value.source_id!==batch.source_id)continue;
        importedTasks++;const events=batch.events.filter(e=>e.source_task_id===value.source_task_id);
        assert.equal(events.length,7);for(const e of events.filter(e=>['user','assistant'].includes(e.kind))) assert.ok(value.messages.some(m=>m.role===e.kind&&m.content===e.content));
        if(value.source_task_id===batch.events[0].source_task_id) first=id;
        const trajectory=await request('GET',`/api/console/applications/${apps[1].id}/logs/records/${id}/client-trajectory?limit=100`);
        assert.equal(trajectory.items.length,events.length);
        for(const event of events) {
          const step=trajectory.items.find(s=>s.item_id===event.event_id);assert.ok(step,'original event identity');
          const raw=await request('GET',`/api/console/applications/${apps[1].id}/logs/records/${id}/client-trajectory/${step.id}?limit=8&section=raw`);
          assert.deepEqual(raw.items[0].value,event,'complete exact source fact');
        }
      }
      assert.equal(importedTasks,3);assert.ok(first);return first;
    }
    receipt.trace_ids=await capture({args,credentials,langfuse,sources,target:{application_id:apps[1].id,name:apps[1].name},receipt,waitRemote,verifyTarget});
    receipt.result='pass';checkpoint();process.stdout.write(JSON.stringify({result:'pass',receipt:path.join(args.outDir,'receipt.json'),traces:receipt.trace_ids.length,screenshots:receipt.screenshots.length})+'\n');
  } catch(error) {receipt.result='fail';receipt.error=error.message;checkpoint();throw error;}
  finally {try {for(const key of keys)await request('DELETE',`/api/console/applications/${key.application_id}/api-keys/${key.id}`);}finally{await owner.dispose();}}
}
main().catch(e=>{process.stderr.write(e.message+'\n');process.exitCode=1;});
