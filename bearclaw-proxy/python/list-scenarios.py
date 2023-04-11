#!/usr/bin/env python3

import capnp
import sys

capnp.remove_import_hook()
bearclaw_capnp = capnp.load('../../bearclaw.capnp')

print('Connecting to bearclaw-proxy RPC...')

client = capnp.TwoPartyClient('localhost:3092')
bearclaw = client.bootstrap().cast_as(bearclaw_capnp.Bearclaw)

print('')
print('Listing scenarios...')
print('')

def printScenario(depth, s):
    print('   ' * depth, s.info.id, s.info.description, s.info.type)
    for child in s.children:
        printScenario(depth + 1, child)

scenarios = bearclaw.listScenarios().wait().list

for s in scenarios:
    printScenario(0, s)