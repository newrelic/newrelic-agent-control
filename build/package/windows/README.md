# Notes

Copy the `newrelic-agent-control.exe` binary to this folder.

```powershell
PS> .\install.ps1 -ServiceOverwrite
Service 'newrelic-agent-control' already exists. Stopping and removing...
Installing New Relic Agent Control Version: development
Creating directories...
Copying New Relic Agent Control program files...
Installing New Relic Agent Control service...
Installation completed!

PS> Get-Service -name newrelic-agent-control

Status   Name               DisplayName
------   ----               -----------
Running  newrelic-agent-... New Relic Agent Control

PS> curl http://localhost:51200/status


StatusCode        : 200
StatusDescription : OK
Content           : {"agent_control":{"healthy":true},"fleet":{"enabled":false,"endpoint":null,"reachable":false},"sub_
                    agents":{}}
RawContent        : HTTP/1.1 200 OK
                    Content-Length: 110
                    Content-Type: application/json
                    Date: Thu, 20 Nov 2025 14:52:47 GMT

                    {"agent_control":{"healthy":true},"fleet":{"enabled":false,"endpoint":null,"reachable":false...
Forms             : {}
Headers           : {[Content-Length, 110], [Content-Type, application/json], [Date, Thu, 20 Nov 2025 14:52:47 GMT]}
Images            : {}
InputFields       : {}
Links             : {}
ParsedHtml        : System.__ComObject
RawContentLength  : 110

PS> Stop-Service -name newrelic-agent-control
```

Delete the service:

```powershell
PS> $serviceToRemove = Get-WmiObject -Class Win32_Service -Filter "name='newrelic-agent-control'"
PS> $serviceToRemove.delete()
PS> Get-Service -name newrelic-agent-control
Get-Service : Cannot find any service with service name 'newrelic-agent-control'.
At line:1 char:1
+ Get-Service -name newrelic-agent-control
+ ~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~~
    + CategoryInfo          : ObjectNotFound: (newrelic-agent-control:String) [Get-Service], ServiceCommandException
    + FullyQualifiedErrorId : NoServiceFoundForGivenName,Microsoft.PowerShell.Commands.GetServiceCommand
```
