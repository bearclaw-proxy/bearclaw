#!/usr/bin/env python3

import capnp
import os
import sys

if len(sys.argv) != 5:
    print('Usage:', sys.argv[0], 'parent-scenario-id new-scenario-id scenario-description scenario-type', file=sys.stderr)
    sys.exit(1)

parentScenarioId = sys.argv[1]
newScenarioId = sys.argv[2]
description = sys.argv[3]
type = sys.argv[4]

capnp.remove_import_hook()
bearclaw_capnp = capnp.load('../../bearclaw.capnp')

print('Connecting to bearclaw-proxy RPC...')

client = capnp.TwoPartyClient(os.environ.get('BEARCLAW_RPC_ENDPOINT', 'localhost:3092'))
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

def createChild(id, description, type, parent):
    info = bearclaw_capnp.NewScenarioInfo.new_message()
    info.id = id
    info.description = description
    info.type = type
    unwrap(parent.createChildScenario(info).wait().result)


print('')
print('Looking up parent scenario...')
print('')

parent = getScenario(parentScenarioId)

print('')
print('Creating scenario...')
print('')

createChild(newScenarioId, description, type, parent)
