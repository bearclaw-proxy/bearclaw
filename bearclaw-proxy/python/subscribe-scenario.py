#!/usr/bin/env python3

import capnp
import sys
import time

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

print('')
print('Looking up scenario...')
print('')

scenario = unwrap(bearclaw.getScenario(scenarioId).wait().result)

print('Subscribing to scenario...')
print('')

scenarioUpdated = False

class ScenarioSubscriberImpl(bearclaw_capnp.ScenarioSubscriber.Server):
    def notifyScenarioUpdated(self, **kwargs):
        global scenarioUpdated
        scenarioUpdated = True

subscriber = ScenarioSubscriberImpl()
subscription = scenario.subscribeScenario(subscriber).wait().subscription

print('Waiting for events...')
print('')

while True:
    capnp.poll_once()
    time.sleep(0.25)
    if scenarioUpdated:
        print('Scenario Updated')
        scenarioUpdated = False