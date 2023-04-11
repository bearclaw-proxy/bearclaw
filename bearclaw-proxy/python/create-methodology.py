#!/usr/bin/env python3

import capnp
import sys

capnp.remove_import_hook()
bearclaw_capnp = capnp.load('../../bearclaw.capnp')

print('Connecting to bearclaw-proxy RPC...')

client = capnp.TwoPartyClient('localhost:3092')
bearclaw = client.bootstrap().cast_as(bearclaw_capnp.Bearclaw)

print('')
print('Creating scenarios...')
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

parent = createRoot('WSTG-v42-INFO', 'Information Gathering', 'container')
createChild('WSTG-v42-INFO-01', 'Conduct Search Engine Discovery Reconnaissance for Information Leakage', 'generic', parent)
createChild('WSTG-v42-INFO-02', 'Fingerprint Web Server', 'generic', parent)
createChild('WSTG-v42-INFO-03', 'Review Webserver Metafiles for Information Leakage', 'generic', parent)
createChild('WSTG-v42-INFO-04', 'Enumerate Applications on Webserver', 'generic', parent)
createChild('WSTG-v42-INFO-05', 'Review Webpage Content for Information Leakage', 'generic', parent)
createChild('WSTG-v42-INFO-06', 'Identify Application Entry Points', 'generic', parent)
createChild('WSTG-v42-INFO-07', 'Map Execution Paths Through Application', 'generic', parent)
createChild('WSTG-v42-INFO-08', 'Fingerprint Web Application Framework', 'generic', parent)
createChild('WSTG-v42-INFO-09', 'Fingerprint Web Application', 'generic', parent)
createChild('WSTG-v42-INFO-10', 'Map Application Architecture', 'generic', parent)

parent = createRoot('WSTG-v42-CONF', 'Configuration and Deployment Management', 'container')
createChild('WSTG-v42-CONF-01', 'Network Infrastructure Configuration', 'generic', parent)
createChild('WSTG-v42-CONF-02', 'Application Platform Configuration', 'generic', parent)
createChild('WSTG-v42-CONF-03', 'File Extensions Handling for Sensitive Information', 'generic', parent)
createChild('WSTG-v42-CONF-04', 'Review Old Backup and Unreferenced Files for Sensitive Information', 'generic', parent)
createChild('WSTG-v42-CONF-05', 'Enumerate Infrastructure and Application Admin Interfaces', 'generic', parent)
createChild('WSTG-v42-CONF-06', 'HTTP Methods', 'generic', parent)
createChild('WSTG-v42-CONF-07', 'HTTP Strict Transport Security', 'generic', parent)
createChild('WSTG-v42-CONF-08', 'RIA Cross Domain Policy', 'generic', parent)
createChild('WSTG-v42-CONF-09', 'File Permission', 'generic', parent)
createChild('WSTG-v42-CONF-10', 'Subdomain Takeover', 'generic', parent)
createChild('WSTG-v42-CONF-11', 'Cloud Storage', 'generic', parent)

parent = createRoot('WSTG-v42-IDNT', 'Identity Management', 'container')
createChild('WSTG-v42-IDNT-01', 'Role Definitions', 'generic', parent)
createChild('WSTG-v42-IDNT-02', 'User Registration Process', 'generic', parent)
createChild('WSTG-v42-IDNT-03', 'Account Provisioning Process', 'generic', parent)
createChild('WSTG-v42-IDNT-04', 'Account Enumeration and Guessable User Account', 'generic', parent)
createChild('WSTG-v42-IDNT-05', 'Weak or Unenforced Username Policy', 'generic', parent)

parent = createRoot('WSTG-v42-ATHN', 'Authentication', 'container')
createChild('WSTG-v42-ATHN-01', 'Credentials Transported over an Encrypted Channel', 'generic', parent)
createChild('WSTG-v42-ATHN-02', 'Default Credentials', 'generic', parent)
createChild('WSTG-v42-ATHN-03', 'Weak Lock Out Mechanism', 'generic', parent)
createChild('WSTG-v42-ATHN-04', 'Bypassing Authentication Schema', 'generic', parent)
createChild('WSTG-v42-ATHN-05', 'Vulnerable Remember Password', 'generic', parent)
createChild('WSTG-v42-ATHN-06', 'Browser Cache Weaknesses', 'generic', parent)
createChild('WSTG-v42-ATHN-07', 'Weak Password Policy', 'generic', parent)
createChild('WSTG-v42-ATHN-08', 'Weak Security Question Answer', 'generic', parent)
createChild('WSTG-v42-ATHN-09', 'Weak Password Change or Reset Functionalities', 'generic', parent)
createChild('WSTG-v42-ATHN-10', 'Weaker Authentication in Alternative Channel', 'generic', parent)

parent = createRoot('WSTG-v42-ATHZ', 'Authorization', 'container')
createChild('WSTG-v42-ATHZ-01', 'Directory Traversal File Include', 'generic', parent)
createChild('WSTG-v42-ATHZ-02', 'Bypassing Authorization Schema', 'generic', parent)
createChild('WSTG-v42-ATHZ-03', 'Privilege Escalation', 'generic', parent)
createChild('WSTG-v42-ATHZ-04', 'Insecure Direct Object References', 'generic', parent)

parent = createRoot('WSTG-v42-SESS', 'Session Management', 'container')
createChild('WSTG-v42-SESS-01', 'Session Management Schema', 'generic', parent)
createChild('WSTG-v42-SESS-02', 'Cookies Attributes', 'generic', parent)
createChild('WSTG-v42-SESS-03', 'Session Fixation', 'generic', parent)
createChild('WSTG-v42-SESS-04', 'Exposed Session Variables', 'generic', parent)
createChild('WSTG-v42-SESS-05', 'Cross Site Request Forgery', 'generic', parent)
createChild('WSTG-v42-SESS-06', 'Logout Functionality', 'generic', parent)
createChild('WSTG-v42-SESS-07', 'Session Timeout', 'generic', parent)
createChild('WSTG-v42-SESS-08', 'Session Puzzling', 'generic', parent)
createChild('WSTG-v42-SESS-09', 'Session Hijacking', 'generic', parent)

parent = createRoot('WSTG-v42-INPV', 'Input Validation', 'container')
createChild('WSTG-v42-INPV-01', 'Reflected Cross Site Scripting', 'generic', parent)
createChild('WSTG-v42-INPV-02', 'Stored Cross Site Scripting', 'generic', parent)
createChild('WSTG-v42-INPV-03', 'HTTP Verb Tampering', 'generic', parent)
createChild('WSTG-v42-INPV-04', 'HTTP Parameter Pollution', 'generic', parent)
createChild('WSTG-v42-INPV-05', 'SQL Injection', 'generic', parent)
createChild('WSTG-v42-INPV-06', 'LDAP Injection', 'generic', parent)
createChild('WSTG-v42-INPV-07', 'XML Injection', 'generic', parent)
createChild('WSTG-v42-INPV-08', 'SSI Injection', 'generic', parent)
createChild('WSTG-v42-INPV-09', 'XPath Injection', 'generic', parent)
createChild('WSTG-v42-INPV-10', 'IMAP SMTP Injection', 'generic', parent)
createChild('WSTG-v42-INPV-11', 'Code Injection', 'generic', parent)
createChild('WSTG-v42-INPV-12', 'Command Injection', 'generic', parent)
createChild('WSTG-v42-INPV-13', 'Format String Injection', 'generic', parent)
createChild('WSTG-v42-INPV-14', 'Incubated Vulnerability', 'generic', parent)
createChild('WSTG-v42-INPV-15', 'HTTP Splitting Smuggling', 'generic', parent)
createChild('WSTG-v42-INPV-16', 'HTTP Incoming Requests', 'generic', parent)
createChild('WSTG-v42-INPV-17', 'Host Header Injection', 'generic', parent)
createChild('WSTG-v42-INPV-18', 'Server-side Template Injection', 'generic', parent)
createChild('WSTG-v42-INPV-19', 'Server-Side Request Forgery', 'generic', parent)

parent = createRoot('WSTG-v42-ERRH', 'Error Handling', 'container')
createChild('WSTG-v42-ERRH-01', 'Improper Error Handling', 'generic', parent)
createChild('WSTG-v42-ERRH-02', 'Stack Traces', 'generic', parent)

parent = createRoot('WSTG-v42-CRYP', 'Weak Cryptography', 'container')
createChild('WSTG-v42-CRYP-01', 'Weak Transport Layer Security', 'generic', parent)
createChild('WSTG-v42-CRYP-02', 'Padding Oracle', 'generic', parent)
createChild('WSTG-v42-CRYP-03', 'Sensitive Information Sent via Unencrypted Channels', 'generic', parent)
createChild('WSTG-v42-CRYP-04', 'Weak Encryption', 'generic', parent)

parent = createRoot('WSTG-v42-BUSL', 'Business Logic', 'container')
createChild('WSTG-v42-BUSL-01', 'Business Logic Data Validation', 'generic', parent)
createChild('WSTG-v42-BUSL-02', 'Ability to Forge Requests', 'generic', parent)
createChild('WSTG-v42-BUSL-03', 'Integrity Checks', 'generic', parent)
createChild('WSTG-v42-BUSL-04', 'Process Timing', 'generic', parent)
createChild('WSTG-v42-BUSL-05', 'Number of Times a Function Can Be Used Limits', 'generic', parent)
createChild('WSTG-v42-BUSL-06', 'Circumvention of Work Flows', 'generic', parent)
createChild('WSTG-v42-BUSL-07', 'Defenses Against Application Misuse', 'generic', parent)
createChild('WSTG-v42-BUSL-08', 'Upload of Unexpected File Types', 'generic', parent)
createChild('WSTG-v42-BUSL-09', 'Upload of Malicious Files', 'generic', parent)

parent = createRoot('WSTG-v42-CLNT', 'Client-side', 'container')
createChild('WSTG-v42-CLNT-01', 'DOM-Based Cross Site Scripting', 'generic', parent)
createChild('WSTG-v42-CLNT-02', 'JavaScript Execution', 'generic', parent)
createChild('WSTG-v42-CLNT-03', 'HTML Injection', 'generic', parent)
createChild('WSTG-v42-CLNT-04', 'Client-side URL Redirect', 'generic', parent)
createChild('WSTG-v42-CLNT-05', 'CSS Injection', 'generic', parent)
createChild('WSTG-v42-CLNT-06', 'Client-side Resource Manipulation', 'generic', parent)
createChild('WSTG-v42-CLNT-07', 'Cross Origin Resource Sharing', 'generic', parent)
createChild('WSTG-v42-CLNT-08', 'Cross Site Flashing', 'generic', parent)
createChild('WSTG-v42-CLNT-09', 'Clickjacking', 'generic', parent)
createChild('WSTG-v42-CLNT-10', 'WebSockets', 'generic', parent)
createChild('WSTG-v42-CLNT-11', 'Web Messaging', 'generic', parent)
createChild('WSTG-v42-CLNT-12', 'Browser Storage', 'generic', parent)
createChild('WSTG-v42-CLNT-13', 'Cross Site Script Inclusion', 'generic', parent)

parent = createRoot('WSTG-v42-APIT', 'API', 'container')
createChild('WSTG-v42-APIT-01', 'GraphQL', 'generic', parent)
