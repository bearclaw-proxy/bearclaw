#!/usr/bin/env python3

import capnp
import sys
import time

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

print('Subscribing to methodology...')
print('')

methodologyUpdated = False

class MethodologySubscriberImpl(bearclaw_capnp.MethodologySubscriber.Server):
    def notifyScenarioTreeChanged(self, **kwargs):
        global methodologyUpdated
        methodologyUpdated = True

subscriber = MethodologySubscriberImpl()
subscription = bearclaw.subscribeMethodology(subscriber).wait().subscription

print('Waiting for events...')
print('')

while True:
    capnp.poll_once()
    time.sleep(0.25)
    if methodologyUpdated:
        print('Methodology Updated')
        methodologyUpdated = False