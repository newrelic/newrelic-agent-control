NRQL Api requests

After executing this role the fact `results` will contain the results.

Example:
```yaml
- name: Graphql Request
  include_role:
    name: graphql_request
```

Vars:
```yaml
# New Relic account ID
nr_account_id: 123456789
# New Relic User API Key
nr_api_key: "xxxxxxxxxxxxxxxx"
graphql_query: |
    {
      account  {
        agents{
          results{
            type
            healthy
            authorization{
              state
              lastUpdated
            }
            host{
              name
            }
            uid
          }
        }
      }
    }
retries: 3
delay: 4
until: response.json is defined and response.json.data.deployFleet.fleetGuid is defined

```