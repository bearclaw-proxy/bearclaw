#!/usr/bin/env python3

import capnp
import sys

if len(sys.argv) != 4:
    print('Usage:', sys.argv[0], 'old-scenario-id new-scenario-id scenario-description', file=sys.stderr)
    sys.exit(1)

scenarioId = sys.argv[1]
newScenarioId = sys.argv[2]
scenarioDescription = sys.argv[3]

capnp.remove_import_hook()
bearclaw_capnp = capnp.load('../../bearclaw.capnp')

print('Connecting to bearclaw-proxy RPC...')

client = capnp.TwoPartyClient('localhost:3092')
bearclaw = client.bootstrap().cast_as(bearclaw_capnp.Bearclaw)

print('')
print('Looking up scenario...')
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

scenario = unwrap(bearclaw.getScenario(scenarioId).wait().result)

print('Getting existing scenario info for timestamp...')
print('')

info = unwrap(scenario.getInfo().wait().result)

print('Updating scenario...')
print('')

updatedInfo = bearclaw_capnp.UpdateScenarioInfo.new_message()
updatedInfo.id = newScenarioId
updatedInfo.description = scenarioDescription
updatedInfo.previousModifiedTimestamp = info.modifiedTimestamp

#unwrap(scenario.delete().wait().result)
unwrap(scenario.updateInfo(updatedInfo).wait().result)
#unwrap(scenario.updateInfo(updatedInfo).wait().result)
