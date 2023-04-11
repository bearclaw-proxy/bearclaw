#!/usr/bin/env python3

import capnp
import sys

capnp.remove_import_hook()
bearclaw_capnp = capnp.load('../../bearclaw.capnp')

print('Connecting to bearclaw-proxy RPC...')

client = capnp.TwoPartyClient('localhost:3092')
bearclaw = client.bootstrap().cast_as(bearclaw_capnp.Bearclaw)

print('')
print('Testing scenario recursion limit...')
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

def createRoot(id, description, type):
    print('', id, description, type)
    info = bearclaw_capnp.NewScenarioInfo.new_message()
    info.id = id
    info.description = description
    info.type = type
    unwrap(bearclaw.createScenario(info).wait().result)
    return getScenario(id)

def createChild(id, description, type, parent):
    print('   ', id, description, type)
    info = bearclaw_capnp.NewScenarioInfo.new_message()
    info.id = id
    info.description = description
    info.type = type
    unwrap(parent.createChildScenario(info).wait().result)

def testRecursionLimit():
    cur = createRoot('recursion-test-root', 'test', 'container')
    for i in range(100):
        testName = 'test' + str(i)
        createChild(testName, testName, 'container', cur)
        cur = getScenario(testName)

testRecursionLimit()
