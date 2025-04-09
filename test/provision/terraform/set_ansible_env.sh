#!/bin/sh

# Setting up SSH for pulling private roles
echo "Setting up SSH for pulling private roles"

eval "$(ssh-agent -s)"

echo "Setting up Ansible environment"
ansible-galaxy role install -r "${REQUIREMENTS_FILE}" -p "${ROLES_PATH}"
ansible-galaxy collection install -r "${REQUIREMENTS_FILE}" -p "${COLLECTIONS_PATH}"

