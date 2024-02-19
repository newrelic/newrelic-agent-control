#!/bin/sh

# Setting up SSH for pulling private roles
echo "Setting up SSH for pulling private roles"

eval "$(ssh-agent -s)"

if ssh-add "$CROWDSTRIKE_ROLE_PULL_KEY"; then
  echo "Added SSH key at ${CROWDSTRIKE_ROLE_PULL_KEY}"
else
  echo "No additional ssh identities added"
fi

echo "Setting up Ansible environment"
ansible-galaxy role install -r "${REQUIREMENTS_FILE}" -p "${ROLES_PATH}"
ansible-galaxy collection install -r "${REQUIREMENTS_FILE}" -p "${COLLECTIONS_PATH}"

