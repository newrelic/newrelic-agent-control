Fleet Api requests

Variables for all actions
```yaml
# New Relic account ID
nr_account_id: 123456789
# New Relic User API Key
nr_api_key: "xxxxxxxxxxxxxxxx"
```


## Agents health assertions
```yaml
    - name: Assert agents exist and are healthy
      include_role:
        name: caos.ansible_roles.fleet_api_request
      vars:
        assert_agents_health:
          instance_ids: ["01HNN91DF9XE69BRYPK9DPHD34", "01HNN91C5XT16VQJ31J3TC7MYM"]
          host: "some-host.example.com"
          healthy: true

    - name: Assert agents exist and are healthy
      include_role:
        name: caos.ansible_roles.fleet_api_request
      vars:
        assert_agents_health:
          instance_ids: ["01HNN91CWC2D50AA4M8QXMH026", "01HNN6Y5J0WWAXSTK413Q27EFJ"]
          host: "some-host.example.com"
          healthy: false

```


## Create Configuration
```yaml
  - name: create remote configuration
    include_role:
      name: caos.ansible_roles.fleet_api_request
    vars:
      create_remote_configuration:
        account_id: "{{ nr_account_id | int }}"
        config_name: "{{ remote_config_name }}"
        config_id_fact: "created_config_id"
```

## Create Configuration Revision
```yaml
  - name: create remote configuration revision (only infra agent)
    include_role:
      name: caos.ansible_roles.fleet_api_request
    vars:
      create_remote_configuration_revision:
        account_id: "{{ nr_account_id | int }}"
        config_id: "{{ created_config_id }}"
        # multiline config breaks graphql request
        content: 'agents:\n  nr-infra-agent:\n    agent_type: \"newrelic/com.newrelic.infrastructure_agent:0.0.1\"\n'
        config_revision_fact: "created_config_revision"
```

## Get Agent:
```yaml
- name: get Agent
  include_role:
    name: caos.ansible_roles.fleet_api_request
  vars:
    get_agent:
      agent_instance_id: "{{ agent_instance_id }}"
      agent_fact: "agent_for_hash"
```

## Get Agents:
```yaml
- name: get Agents
  include_role:
    name: caos.ansible_roles.fleet_api_request
  vars:
    get_agents: true
```

## List Fleets:
```yaml
- name: get fleets
  include_role:
    name: caos.ansible_roles.fleet_api_request
  vars:
    list_fleets: true

```
