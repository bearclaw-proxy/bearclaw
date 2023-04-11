#!/usr/bin/env python3

import capnp
import sys

if len(sys.argv) != 2:
    print('Usage:', sys.argv[0], 'scenario-id', file=sys.stderr)
    sys.exit(1)

scenarioId = sys.argv[1]

capnp.remove_import_hook()
bearclaw_capnp = capnp.load('../../bearclaw.capnp')

print('Connecting to bearclaw-proxy RPC...')

client = capnp.TwoPartyClient('localhost:3092')
bearclaw = client.bootstrap().cast_as(bearclaw_capnp.Bearclaw)

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
    
print('')
print('Looking up scenario...')
print('')

scenario = getScenario(scenarioId)

print('Deleting scenario...')
print('')

unwrap(scenario.delete().wait().result)
