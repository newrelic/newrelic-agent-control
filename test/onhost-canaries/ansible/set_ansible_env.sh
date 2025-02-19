#!/bin/sh

# Setting up SSH for pulling private roles
echo "Setting up SSH for pulling private roles"

eval "$(ssh-agent -s)"

echo "Setting up Ansible environment"
ansible-galaxy collection install -r "${REQUIREMENTS_FILE}" -p "${COLLECTIONS_PATH}"

