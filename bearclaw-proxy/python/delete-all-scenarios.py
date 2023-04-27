#!/usr/bin/env python3

import capnp
import os
import sys

capnp.remove_import_hook()
bearclaw_capnp = capnp.load('../../bearclaw.capnp')

print('Connecting to bearclaw-proxy RPC...')

client = capnp.TwoPartyClient(os.environ.get('BEARCLAW_RPC_ENDPOINT', 'localhost:3092'))
bearclaw = client.bootstrap().cast_as(bearclaw_capnp.Bearclaw)

print('')
print('Deleting scenarios...')
print('')

def unwrap(o):
    if o.which() == 'ok':
        return o.ok
    elif o.which() == 'some':
        return o.some
    elif o.which() == 'success':
        return
    else:
        print(o, file=sys.stderr)
        sys.exit(1)

def getScenario(id):
    return unwrap(bearclaw.getScenario(id).wait().result);

scenarios = bearclaw.listScenarios().wait().list

for s in scenarios:
    print('Deleting:', s.info.id, s.info.description, s.info.type)
    sObj = getScenario(s.info.id)
    unwrap(sObj.delete().wait().result)
    #unwrap(sObj.delete().wait().result)
