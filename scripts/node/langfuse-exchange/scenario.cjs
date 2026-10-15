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
const { readTraceObservations } = require('./remote-readback.cjs');
const { verifyOpenApi } = require('./openapi.cjs');
const root = path.resolve(__dirname,'../../..');
async function main() {
  const { values } = parseArgs({options:{'api-base-url':{type:'string'},'web-base-url':{type:'string'},'credentials-root':{type:'string'},'langfuse-private-file':{type:'string'},'langfuse-storage-private-file':{type:'string'},'out-dir':{type:'string'},'native-application-id':{type:'string'},'deadline-ms':{type:'string',default:'120000'}}});
  for(const field of ['api-base-url','web-base-url','credentials-root','langfuse-private-file','langfuse-storage-private-file','out-dir']) assert.ok(values[field],`missing ${field}`);
  const args=Object.fromEntries(Object.entries(values).map(([k,v])=>[k.replace(/-([a-z])/g,(_,c)=>c.toUpperCase()),v]));
  args.deadlineMs=Number(args.deadlineMs);args.outDir=path.resolve(args.outDir);
  for(const url of [args.apiBaseUrl,args.webBaseUrl]) assert.ok(!['7600','7800'].includes(new URL(url).port),'independent candidate only');
  assert.ok(args.outDir.startsWith(path.join(root,'tmp/test-governance/')),'evidence stays within candidate governance');
  fs.mkdirSync(path.join(args.outDir,'private'),{recursive:true,mode:0o700});
  const receipt={source_sha:execFileSync('git',['rev-parse','HEAD'],{cwd:root,encoding:'utf8'}).trim(),run_id:crypto.randomUUID().slice(0,8),result:'incomplete',checks:[],exports:[],imports:[],screenshots:[],created_application_ids:[],created_application_api_keys:[],created_instance_ids:[],created_user_api_key_ids:[]};
  const checkpoint=()=>fs.writeFileSync(path.join(args.outDir,'receipt.json'),JSON.stringify(receipt,null,2));
  const credentials=loadRootCredentials({repoRoot:args.credentialsRoot});
  const owner=await openTemporaryOwnerSession({apiBaseUrl:args.apiBaseUrl,...credentials});
  const keys=[], userKeys=[];
  let failure, originalLocale, existingInstances;
  async function request(method,route,body,token) {
    const response=await fetch(args.apiBaseUrl+route,{method,redirect:'error',signal:AbortSignal.timeout(args.deadlineMs),headers:{'content-type':'application/json',...(token?{Authorization:`Bearer ${token}`}:{cookie:owner.cookie,'x-csrf-token':owner.csrfToken})},...(body===undefined?{}:{body:JSON.stringify(body)})});
    const text=await response.text(); let result;
    try { result=text ? JSON.parse(text) : undefined; }
    catch { throw Error(`HTTP ${response.status} invalid JSON ${route}`); }
    assert.ok(response.ok,`HTTP ${response.status} ${route}: ${result?.code||'unknown'}`);
    if (response.status === 204 && method === 'DELETE') return;
    assert.ok(result, `HTTP ${response.status} missing JSON ${route}`);
    return route.endsWith('/otlp/export')?result:result.data;
  }
  async function listInstances() {
    const items=[]; let page=1,total;
    do {
      const value=await request('GET',`/api/console/managed-services/langfuse/instances?page=${page}&page_size=50`);
      assert.equal(value.success,true); assert.equal(value.page,page);
      total=value.total; items.push(...value.items);
      assert.equal(new Set(items.map(item=>item.instance_id)).size,items.length,'instance pagination progresses');
      assert.ok(value.items.length>0 || items.length===total,'instance pagination complete'); page++;
    } while(items.length<total);
    assert.equal(items.length,total);return items;
  }
  try {
    const me=await request('GET','/api/console/me');
    originalLocale={preferred_locale:me.preferred_locale,meta_preferred_locale:me.meta?.ui?.locale?.preferred_locale};
    receipt.original_locale=originalLocale;checkpoint();
    existingInstances=await listInstances();
    const langfuse=JSON.parse(fs.readFileSync(args.langfusePrivateFile,'utf8'));langfuse.base_url=langfuse.base_url.replace(/\/$/,'');
    const suffix=receipt.run_id;
    const apps=[];
    for(const label of ['Langfuse plugin source','Langfuse plugin imported']) {
      const app=await request('POST','/api/console/applications',{application_type:'agent_logs',name:`${label} ${suffix}`,description:'Public Langfuse plugin browser acceptance fixture',icon:null,icon_type:null,icon_background:null});
      apps.push(app);receipt.created_application_ids.push(app.id);checkpoint();
      await request('PATCH',`/api/console/applications/${app.id}/api-status`,{api_enabled:true});
      const key=await request('POST',`/api/console/applications/${app.id}/api-keys`,{name:`langfuse-${suffix}`,expires_at:null});keys.push({application_id:app.id,...key});
      receipt.created_application_api_keys.push({application_id:app.id,id:key.id});checkpoint();
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
    async function waitRemote(traceIds, expectedSpans) {
      const deadline=Date.now()+args.deadlineMs;
      for (;;) {
        const summary={}; let count=0;
        for (const id of traceIds) {
          const rows=await readTraceObservations({baseUrl:langfuse.base_url,authorization,traceId:id,deadlineMs:Math.max(1,deadline-Date.now())});
          const starts=rows.map(row=>Date.parse(row.startTime));
          assert.ok(starts.every(Number.isFinite),'remote actual observation start times');
          summary[id]={observation_ids:rows.map(row=>row.id).sort(),start_time:rows.length ? new Date(starts.reduce((first, at)=>Math.min(first,at),Infinity)).toISOString() : null};
          count+=rows.length;
        }
        if(Object.values(summary).every(trace=>trace.observation_ids.length) && count>=expectedSpans) {
          receipt.remote_readback=summary;checkpoint();return summary;
        }
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
    receipt.trace_ids=await capture({args,credentials,langfuse,sources,target:{application_id:apps[1].id,name:apps[1].name},receipt,waitRemote,verifyTarget,checkpoint,request,
      verifyInstance:instanceId=>verifyOpenApi({request,args,instanceId,secrets:[langfuse.public_key,langfuse.secret_key,Buffer.from(`${langfuse.public_key}:${langfuse.secret_key}`).toString('base64')],receipt,checkpoint,userKeys})});
    assert.equal(receipt.screenshots.length,20,'original 14 safe screenshots plus bilingual instance table/drawer evidence');
    receipt.result='pass';checkpoint();
  } catch(error) {receipt.result='fail';receipt.error=error.message;failure=error;checkpoint();}
  finally {
    receipt.cleanup={revoked_keys:[],revoked_user_api_keys:[],deleted_instance_ids:[],errors:[]};
    for(const key of keys) {
      try {await request('DELETE',`/api/console/applications/${key.application_id}/api-keys/${key.id}`);receipt.cleanup.revoked_keys.push(key.id);}
      catch(error) {receipt.cleanup.errors.push(error.message);}
    }
    for(const key of userKeys) {
      try {await request('POST',`/api/console/user-api-keys/${key.id}/revoke`);}
      catch(error) {receipt.cleanup.errors.push(error.message);}
      // Verification is independent of the revoke result and never stores the token.
      try {
        const listed=await request('GET','/api/console/user-api-keys');
        assert.equal(listed.items.find(item=>item.id===key.id)?.revoked,true,'temporary PAT independently confirmed revoked');
        receipt.cleanup.revoked_user_api_keys.push(key.id);
      } catch(error) {receipt.cleanup.errors.push(error.message);}
    }
    for(const id of receipt.created_instance_ids) {
      try {
        const listed=await listInstances();
        if(listed.some(item=>item.instance_id===id)) await request('DELETE',`/api/console/managed-services/langfuse/instances/${encodeURIComponent(id)}`);
        const after=await listInstances();
        assert.ok(!after.some(item=>item.instance_id===id),'owned instance removed');
        receipt.cleanup.deleted_instance_ids.push(id);
      } catch(error) {receipt.cleanup.errors.push(error.message);}
    }
    if(existingInstances) {
      try {
        const after=await listInstances();
        for(const original of existingInstances) assert.deepEqual(after.find(item=>item.instance_id===original.instance_id),original,'preexisting connections unchanged');
        receipt.cleanup.preexisting_instances_preserved=true;
      } catch(error) {receipt.cleanup.errors.push(error.message);}
    }
    if(originalLocale) {
      try {
        const current=await request('GET','/api/console/me');
        // The API uses JSON Merge Patch. Null removes an originally absent owned
        // preference; current metadata preserves unrelated concurrent preferences.
        const meta={...current.meta,ui:{...current.meta?.ui,locale:{...current.meta?.ui?.locale,preferred_locale:originalLocale.meta_preferred_locale ?? null}}};
        await request('PATCH','/api/console/me/meta',{meta});
        if(current.preferred_locale!==originalLocale.preferred_locale) {
          const body=Object.fromEntries(['name','nickname','email','phone','avatar_url','introduction'].map(field=>[field,current[field]]));
          await request('PATCH','/api/console/me',{...body,preferred_locale:originalLocale.preferred_locale ?? null});
        }
        const restored=await request('GET','/api/console/me');
        assert.equal(restored.meta?.ui?.locale?.preferred_locale,originalLocale.meta_preferred_locale);
        assert.equal(restored.preferred_locale,originalLocale.preferred_locale);
        receipt.cleanup.locale_restored=true;
      } catch(error) {receipt.cleanup.errors.push(error.message);}
    }
    try {await owner.dispose();receipt.cleanup.session_revoked=true;}
    catch(error) {receipt.cleanup.errors.push(error.message);}
    if(receipt.cleanup.errors.length && !failure) {
      failure=Error('Acceptance cleanup failed; see receipt cleanup errors');receipt.result='fail';receipt.error=failure.message;
    }
    checkpoint();
  }
  if(failure)throw failure;
  process.stdout.write(JSON.stringify({result:'pass',receipt:path.join(args.outDir,'receipt.json'),traces:receipt.trace_ids.length,screenshots:receipt.screenshots.length})+'\n');
}
main().catch(e=>{process.stderr.write(e.message+'\n');process.exitCode=1;});
