import json, urllib.request, urllib.error, sys, time, re
repo=sys.argv[1] if len(sys.argv)>1 else '/Users/hugues_mini/Codes/AgentTools/agent-context-engine'
mode=sys.argv[2] if len(sys.argv)>2 else 'repo'
token=len(sys.argv)>3 and sys.argv[3]=='token'
slug=''.join(c if c.isalnum() else '_' for c in repo).strip('_')[-64:]
url='http://127.0.0.1:6699/'+('mcp-repo/'+slug if mode=='repo' else 'mcp')
headers={'Content-Type':'application/json','Accept':'application/json, text/event-stream'}
def call(body):
 start=time.monotonic()
 req=urllib.request.Request(url,json.dumps(body).encode(),headers)
 try:
  with urllib.request.urlopen(req, timeout=65) as r:
   if r.headers.get('mcp-session-id'): headers['mcp-session-id']=r.headers['mcp-session-id']
   print(body['method'],'HTTP',r.status,flush=True)
   if r.status==202: return {}
   if 'text/event-stream' in r.headers.get('Content-Type',''):
    for line in r:
     if line.startswith(b'data:') and line[5:].strip():
      item=json.loads(line[5:])
      if item.get('method'): print('notification:',json.dumps(item),flush=True)
      if item.get('id')==body.get('id'):
       print('response',round(time.monotonic()-start,2),'s',json.dumps(item)[:2500],flush=True)
       return item
   else:
    item=json.load(r);print('response',json.dumps(item)[:2500],flush=True);return item
 except urllib.error.HTTPError as e:
  print('HTTP FAILURE',e.code,e.read().decode()[:1500],flush=True);raise SystemExit(1)
 raise AssertionError('Stream ended without response')
print('TEST',mode,'progress token',token,'repo',repo,flush=True)
a=call({'jsonrpc':'2.0','id':1,'method':'initialize','params':{'protocolVersion':'2024-11-05','capabilities':{},'clientInfo':{'name':'merge-repro','version':'1'}}})
assert 'result' in a,a
call({'jsonrpc':'2.0','method':'notifications/initialized'})
a=call({'jsonrpc':'2.0','id':2,'method':'tools/list','params':{}})
assert any(t['name']=='codebase-retrieval' for t in a['result']['tools'])
args={'information_request':'Where is the application entry point and how does it start the server?'}
if mode!='repo': args['workspace_full_path']=repo
params={'name':'codebase-retrieval','arguments':args}
if token: params['_meta']={'progressToken':'repro-3'}
a=call({'jsonrpc':'2.0','id':3,'method':'tools/call','params':params})
assert 'error' not in a and 'result' in a and not a['result'].get('isError'),a
texts='\n'.join(c.get('text','') for c in a['result'].get('content',[]))
assert re.search(r'#L[0-9]+',texts),texts[:1000]
print('PASS retrieval returned content',flush=True)
