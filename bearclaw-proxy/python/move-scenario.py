#!/usr/bin/env python3

import capnp
import os
import sys

if len(sys.argv) != 4:
    print('Usage:', sys.argv[0], 'scenario-id [before | after | inside] target-scenario-id', file=sys.stderr)
    sys.exit(1)

scenarioId = sys.argv[1]
targetPosition = sys.argv[2].lower()
targetScenarioId = sys.argv[3]

if targetPosition not in ["before", "after", "inside"]:
    print('Usage:', sys.argv[0], 'scenario-id [before | after | inside] target-scenario-id', file=sys.stderr)
    sys.exit(1)

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

print('')
print('Looking up scenarios...')
print('')

source = getScenario(scenarioId)
dest = getScenario(targetScenarioId)

print('Moving scenario...')
print('')

if targetPosition == 'before':
    unwrap(source.moveBefore(dest).wait().result)
elif targetPosition == 'after':
    unwrap(source.moveAfter(dest).wait().result)
elif targetPosition == 'inside':
    unwrap(source.moveInside(dest).wait().result)
