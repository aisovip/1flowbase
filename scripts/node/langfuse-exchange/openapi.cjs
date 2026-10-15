'use strict';
const assert = require('node:assert/strict');
const prefix = '/api/console/managed-services/langfuse';

function noSecrets(value, secrets) {
  const text = JSON.stringify(value);
  for (const secret of secrets) if (secret) assert.ok(!text.includes(secret), 'response must not expose credentials');
}
function checkResponse(result, secrets, expectedStatus) {
  assert.equal(result.success, true);
  assert.equal(result.response.status, expectedStatus);
  assert.equal(typeof result.response.headers, 'object');
  assert.equal(typeof result.response.body_base64, 'string');
  const bytes = Buffer.from(result.response.body_base64, 'base64');
  assert.ok(bytes.length, 'actual upstream response body');
  noSecrets(bytes.toString('utf8'), secrets);
  if (Object.hasOwn(result.response, 'body')) assert.deepEqual(JSON.parse(bytes.toString('utf8')), result.response.body);
  noSecrets(result, secrets);
  return bytes;
}
function checkReferences(operation) {
  let references = 0;
  const visit = value => {
    if (!value || typeof value !== 'object') return;
    if (typeof value.$ref === 'string') {
      assert.ok(value.$ref.startsWith('#/components/'), 'actual local component reference');
      const resolved = value.$ref.slice(2).split('/').reduce((node, part) => node?.[part.replace(/~1/g,'/').replace(/~0/g,'~')], operation);
      assert.ok(resolved, 'get includes referenced component closure'); references++;
    }
    for (const child of Object.values(value)) visit(child);
  };
  visit(operation);
  assert.ok(references > 0 && Object.keys(operation.components).length > 0, 'meaningful schema dependency closure');
}
async function verifyOpenApi({request, args, instanceId, secrets, receipt, checkpoint, userKeys}) {
  const post = (operation, body={}) => request('POST', `${prefix}/openapi/${operation}`, {instance_id:instanceId,...body});
  const discovered = await post('discover');
  assert.equal(discovered.success,true); assert.equal(discovered.instance_id,instanceId);
  assert.ok(discovered.operations.length > 0);
  const ids = discovered.operations.map(operation=>operation.operationId);
  assert.equal(new Set(ids).size,ids.length,'operation identities are unique');
  const projects = discovered.operations.find(operation=>operation.method.toUpperCase()==='GET' && operation.path==='/api/public/projects');
  const observations = discovered.operations.find(operation=>operation.method.toUpperCase()==='GET' && operation.path==='/api/public/v2/observations');
  assert.ok(projects && observations,'required actual observed GET metadata');
  const document = await post('get');
  const actual = [];
  for (const [route, item] of Object.entries(document.document.paths)) for (const [method, operation] of Object.entries(item)) {
    if (/^(get|post|put|patch|delete|head|options|trace)$/.test(method)) actual.push({operationId:operation.operationId,method:method.toUpperCase(),path:route});
  }
  assert.deepEqual(discovered.operations.map(({operationId,method,path})=>({operationId,method:method.toUpperCase(),path})).sort((a,b)=>a.operationId.localeCompare(b.operationId)), actual.sort((a,b)=>a.operationId.localeCompare(b.operationId)), 'discovery covers complete live document without fixed operation count');
  const projectSchema = await post('get',{operationId:projects.operationId});
  assert.equal(projectSchema.path,projects.path); assert.equal(projectSchema.operation.operationId,projects.operationId);
  assert.deepEqual(projectSchema.operation,document.document.paths[projects.path].get,'get preserves actual operation schema');
  assert.ok(Array.isArray(projectSchema.parameters));
  const observationSchema = await post('get',{operationId:observations.operationId}); checkReferences(observationSchema);
  assert.deepEqual(observationSchema.operation,document.document.paths[observations.path].get);
  assert.ok(observationSchema.parameters.some(parameter=>parameter.in==='query' && parameter.name==='limit'),'real observation parameter schema');
  const projectResult = await post('execute',{operationId:projects.operationId}); checkResponse(projectResult,secrets,200);
  assert.ok(Array.isArray(projectResult.response.body.data),'actual projects payload');
  const observationResult = await post('execute',{operationId:observations.operationId,query:{limit:1}}); checkResponse(observationResult,secrets,200);
  const invalid = await post('execute',{operationId:observations.operationId,query:{limit:-1}});
  assert.ok(invalid.response.status>=400 && invalid.response.status<500,'invalid limit returns actual upstream 4xx');
  checkResponse(invalid,secrets,invalid.response.status);
  assert.ok(invalid.response.body && typeof invalid.response.body==='object','upstream error JSON preserved');
  noSecrets([discovered,document,projectSchema,observationSchema],secrets);
  receipt.openapi={instance_id:instanceId,operation_count:ids.length,projects_operation:projects.operationId,observations_operation:observations.operationId,projects_status:200,observations_status:200,invalid_limit_status:invalid.response.status}; checkpoint();

  const key = await request('POST','/api/console/user-api-keys',{name:`langfuse-acceptance-${instanceId}`,expiration_policy:'30d'});
  userKeys.push({id:key.id,token:key.token}); receipt.created_user_api_key_ids.push(key.id); checkpoint();
  assert.ok(key.token,'temporary user PAT');
  const catalog = await request('GET','/api/console/mcp/catalog');
  const instance = catalog.instances.find(item=>item.status==='enabled' && item.instance_id!=='frontstage_browser');
  assert.ok(instance,'existing enabled protocol MCP instance');
  let sequence=0;
  async function rpc(name, arguments_) {
    const id=++sequence;
    const response=await fetch(`${args.apiBaseUrl}/api/mcp/${encodeURIComponent(instance.instance_id)}`,{method:'POST',redirect:'error',signal:AbortSignal.timeout(args.deadlineMs),headers:{Authorization:`Bearer ${key.token}`,'content-type':'application/json',Accept:'application/json'},body:JSON.stringify({jsonrpc:'2.0',id,method:'tools/call',params:{name,arguments:arguments_}})});
    assert.ok(response.ok,`MCP protocol HTTP ${response.status}`);
    const envelope=await response.json(); assert.equal(envelope.id,id); assert.ok(!envelope.error,'MCP JSON-RPC succeeds');
    assert.equal(envelope.result.isError,false,'MCP tool succeeds');
    const value=envelope.result.structuredContent; assert.ok(value && typeof value==='object' && !Array.isArray(value),'MCP structuredContent object');
    noSecrets(value,[...secrets,key.token]); return value;
  }
  const listed=await rpc('mcp_list',{path:'/plugins/langfuse',depth:1}); assert.ok(Array.isArray(listed.result),'MCP list array wrapper');
  const calls=[['instances.list',{},'items'],['openapi.discover',{instance_id:instanceId,search:projects.path},'operations'],['openapi.get',{instance_id:instanceId,operationId:projects.operationId},'operationId'],['openapi.execute',{instance_id:instanceId,operationId:projects.operationId},'response']];
  const summaries=[];
  for(const [name,arguments_,field] of calls) {
    const tool_id=`plugin.langfuse.${name}`;
    assert.ok(listed.result.some(item=>item.id===tool_id),'fixed tool is discoverable');
    const description=await rpc('mcp_get',{tool_id}); assert.equal(description.tool_id,tool_id);
    let des_id=description.des_id;
    if(description.description_tool_id) des_id=(await rpc('mcp_get',{tool_id:description.description_tool_id})).des_id;
    const controls={tool_id,arguments:arguments_};
    if(description.des_id_required) {assert.ok(des_id);controls.des_id=des_id;}
    // Current managed tools do not open call controls. Read oversized values through
    // mcp_result's documented JSON-pointer projection, never rerun an operation.
    let value=await rpc('mcp_call',controls);
    if(value.detail) {
      assert.equal(value.outcome,'succeeded'); assert.equal(value.retry_original,false);
      assert.equal(value.detail.status,'continuation_available');
      const result_ref=value.detail.result_ref;
      const fields=['/success',`/${field}`];
      if(name==='openapi.execute') fields.splice(1,1,'/response/status');
      const entries=[]; const cursors=new Set(); let cursor;
      do {
        const page=await rpc('mcp_result',{result_ref,response_fields:fields,max_inline_chars:16000,...(cursor?{cursor}:{})});
        assert.equal(page.detail_status,'available'); assert.ok(!page.selection_error); assert.ok(Array.isArray(page.entries));
        entries.push(...page.entries); cursor=page.next_cursor;
        if(cursor){assert.ok(!cursors.has(cursor),'MCP continuation progresses');cursors.add(cursor);}
      } while(cursor);
      assert.ok(entries.some(entry=>entry.path==='/success' && entry.value===true));
      if(name==='openapi.execute') assert.ok(entries.some(entry=>entry.path==='/response/status' && entry.value===200));
      else if(name==='instances.list') assert.ok(entries.some(entry=>/^\/items\/\d+\/instance_id$/.test(entry.path) && entry.value===instanceId),'selected instance in actual MCP list');
      else if(name==='openapi.discover') assert.ok(entries.some(entry=>/^\/operations\/\d+\/operationId$/.test(entry.path) && entry.value===projects.operationId),'actual project metadata in MCP discover');
      else assert.ok(entries.some(entry=>entry.path==='/operationId' && entry.value===projects.operationId),'actual operation in MCP get');
      value={success:true};
    } else {
      assert.equal(value.success,true);
      assert.ok(Object.hasOwn(value,field));
      if(name==='openapi.execute') assert.equal(value.response.status,200);
      if(name==='instances.list') assert.ok(value.items.some(item=>item.instance_id===instanceId));
      if(name==='openapi.discover') assert.ok(value.operations.some(item=>item.operationId===projects.operationId));
      if(name==='openapi.get') assert.equal(value.operationId,projects.operationId);
    }
    summaries.push({tool_id,description_read:true,called:true});
  }
  receipt.mcp={instance_id:instance.instance_id,tools:summaries}; receipt.checks.push({name:'actual OpenAPI and bearer PAT MCP list/get/call',status:'pass'}); checkpoint();
}
module.exports={verifyOpenApi,noSecrets};
