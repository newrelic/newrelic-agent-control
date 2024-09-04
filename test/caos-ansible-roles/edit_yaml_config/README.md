Edit YAML config

Variables for all actions
```yaml
# Path of the yaml config file
config_path: "/etc/newrelic-super-agent/config.yaml"
# in update_config all the existing keys added will be updated and the new ones will be added
update_config:
  # this will change the host_id
  host_id: "a-host-id"
  # this will add a new config entry called a_new_key
  a_new_key: "a-new_value"
  
```


## Example usage
```yaml
    - name: Setup NR Super Agent config add host id
      include_role:
        name: edit_yaml_config
      vars:
        config_path: "/etc/newrelic-super-agent/config.yaml"
        update_config:
          host_id: "my-host-id"

```
