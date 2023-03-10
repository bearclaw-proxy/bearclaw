#!/usr/bin/env python3

import capnp
import time

capnp.remove_import_hook()
bearclaw_capnp = capnp.load('../../bearclaw.capnp')

def printReceivedAt(result, message):
    print(result)
    return message.connectionInfo()

def printConnectionInfo(result, message):
    print(result)
    return message.requestBytes()

def printRequest(result, message):
    print(result)
    return message.responseBytes()

def printResponse(result):
    print(result)
    print('')

class InterceptedMessageSubscriberImpl(bearclaw_capnp.InterceptedMessageSubscriber.Server):
    def pushMessage(self, message, **kwargs):
        # Sadly we can't use .wait() in callabcks, which makes this code a lot more complicated :(
        # This function returns a promise, so we have to construct a promise that does all the
        # network calls we want and return it. Surely there's a better way to do this?!
        return message.requestTimestamp().then(lambda x: printReceivedAt(x, message)) \
            .then(lambda x: printConnectionInfo(x, message)) \
            .then(lambda x: printRequest(x, message)) \
            .then(lambda x: printResponse(x))

print('Connecting to bearclaw-proxy RPC...')

client = capnp.TwoPartyClient('localhost:3092')
bearclaw = client.bootstrap().cast_as(bearclaw_capnp.Bearclaw)

print('Requesting build info...')
print(bearclaw.buildInfo().wait().buildInfo)

print('Sending HTTP request and waiting for response...')

connInfo = bearclaw_capnp.ConnectionInfo.new_message()
connInfo.host = 'example.com'
connInfo.port = 443
connInfo.isHttps = True

request = b"GET / HTTP/1.1\r\nHost: example.com\r\n\r\n"
print(request)
response = bearclaw.send(connInfo, request).wait().response

if response.which() == 'some':
    print(response.some)
else:
    print('No response returned')

print('')
print('Waiting for intercepted messages...')

subscriber = InterceptedMessageSubscriberImpl()
subscription = bearclaw.intercept(subscriber).wait().subscription

while True:
    capnp.poll_once()
    time.sleep(0.001)
