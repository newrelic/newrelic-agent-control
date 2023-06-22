ec2_otels = {
  "otel-canary:0.0.6-linux-amd64:ubuntu22.04" = {
    ami             = "ami-0aeb7c931a5a61206"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t3a.small"
    username        = "ubuntu"
    python          = "/usr/bin/python3"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-amd64:ubuntu20.04" = {
    ami             = "ami-03b6c8bd55e00d5ed"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t3a.small"
    username        = "ubuntu"
    python          = "/usr/bin/python3"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-amd64:ubuntu18.04" = {
    ami             = "ami-0600b1bef20a0c212"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t3a.small"
    username        = "ubuntu"
    python          = "/usr/bin/python3"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-arm64:ubuntu22.04" = {
    ami             = "ami-0717cbd2f49a61ed0"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t4g.small"
    username        = "ubuntu"
    python          = "/usr/bin/python3"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-arm64:ubuntu20.04" = {
    ami             = "ami-0b0c8ae527978b689"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t4g.small"
    username        = "ubuntu"
    python          = "/usr/bin/python3"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-arm64:ubuntu18.04" = {
    ami             = "ami-09ee3570b9d8bc8cc"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t4g.small"
    username        = "ubuntu"
    python          = "/usr/bin/python3"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-amd64:centos7" = {
    ami             = "ami-00f8e2c955f7ffa9b"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t3a.small"
    username        = "centos"
    python          = "/usr/bin/python"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-amd64:centos8" = {
    ami             = "ami-0ac6967966621d983"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t3a.small"
    username        = "centos"
    python          = "/usr/bin/python3"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-amd64:centos-stream" = {
    ami             = "ami-0d97ef13c06b05a19"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t3a.small"
    username        = "centos"
    python          = "/usr/bin/python3"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-arm64:centos7" = {
    ami             = "ami-07f692d95b2b9c8c5"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t4g.small"
    username        = "centos"
    python          = "/usr/bin/python"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-arm64:centos8" = {
    ami             = "ami-035734c938e7da7af"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t4g.small"
    username        = "centos"
    python          = "/usr/bin/python3"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-arm64:centos-stream" = {
    ami             = "ami-0deb895048c4f105b"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t4g.small"
    username        = "centos"
    python          = "/usr/bin/python3"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-amd64:sles-12.5" = {
    ami             = "ami-05f6084a6b524a8f0"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t3a.small"
    username        = "ec2-user"
    python          = "/usr/bin/python"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-amd64:sles-15.2" = {
    ami             = "ami-0b14d12cfa5e9dcc5"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t3a.small"
    username        = "ec2-user"
    python          = "/usr/bin/python3"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-amd64:sles-15.3" = {
    ami             = "ami-055742ec92937ec3e"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t3a.small"
    username        = "ec2-user"
    python          = "/usr/bin/python3"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-amd64:sles-15.4" = {
    ami             = "ami-0ca19ecee2be612fc"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t3a.small"
    username        = "ec2-user"
    python          = "/usr/bin/python3"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-arm64:sles-15.2" = {
    ami             = "ami-0ccf2efb4e042d474"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t4g.small"
    username        = "ec2-user"
    python          = "/usr/bin/python3"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-arm64:sles-15.3" = {
    ami             = "ami-075a486be6269029f"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t4g.small"
    username        = "ec2-user"
    python          = "/usr/bin/python3"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-arm64:sles-15.4" = {
    ami             = "ami-0885abe5302e6fee0"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t4g.small"
    username        = "ec2-user"
    python          = "/usr/bin/python3"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-amd64:redhat-7.9" = {
    ami             = "ami-0680f7cf1ea8cb793"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t3a.small"
    username        = "ec2-user"
    python          = "/usr/bin/python"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-amd64:redhat-8.4" = {
    ami             = "ami-0ba62214afa52bec7"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t3a.small"
    username        = "ec2-user"
    python          = "/usr/bin/python3"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-amd64:redhat-9.0" = {
    ami             = "ami-078cbc4c2d057c244"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t3a.small"
    username        = "ec2-user"
    python          = "/usr/bin/python3"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-arm64:redhat-7.6" = {
    ami             = "ami-0302c1ecc74930ba5"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t4g.small"
    username        = "ec2-user"
    python          = "/usr/bin/python"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-arm64:redhat-9.0" = {
    ami             = "ami-01089181b0aa3be51"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t4g.small"
    username        = "ec2-user"
    python          = "/usr/bin/python"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-amd64:debian-buster" = {
    ami             = "ami-0f42acddbf04bd1b6"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t3a.small"
    username        = "admin"
    python          = "/usr/bin/python3"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-amd64:debian-bullseye" = {
    ami             = "ami-08a0dab67e025361b"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t3a.small"
    username        = "admin"
    python          = "/usr/bin/python3"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-arm64:debian-buster" = {
    ami             = "ami-07d2054a4befc97f7"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t4g.small"
    username        = "admin"
    python          = "/usr/bin/python3"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-arm64:debian-bullseye" = {
    ami             = "ami-03cabbbc935f5826f"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t4g.small"
    username        = "admin"
    python          = "/usr/bin/python3"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-amd64:al-2" = {
    ami             = "ami-077e31c4939f6a2f3"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t3a.small"
    username        = "ec2-user"
    python          = "/usr/bin/python"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-arm64:al-2" = {
    ami             = "ami-07a3e3eda401f8caa"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t4g.small"
    username        = "ec2-user"
    python          = "/usr/bin/python"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-amd64:al-2022" = {
    ami             = "ami-09a0187acf6bdfe59"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t3a.small"
    username        = "ec2-user"
    python          = "/usr/bin/python3"
    tags            = {
      "otel_role" = "agent"
    }
  }
  "otel-canary:0.0.6-linux-arm64:al-2022" = {
    ami             = "ami-0d20d440a115a6737"
    subnet          = "subnet-09b64de757828cdd4"
    security_groups = ["sg-044ef7bc34691164a"]
    key_name        = "caos-dev-arm"
    instance_type   = "t4g.small"
    username        = "ec2-user"
    python          = "/usr/bin/python3"
    tags            = {
      "otel_role" = "agent"
    }
  }
}

ssh_pub_key      = "AAAAB3NzaC1yc2EAAAADAQABAAABAQDH9C7BS2XrtXGXFFyL0pNku/Hfy84RliqvYKpuslJFeUivf5QY6Ipi8yXfXn6TsRDbdxfGPi6oOR60Fa+4cJmCo6N5g57hBS6f2IdzQBNrZr7i1I/a3cFeK6XOc1G1tQaurx7Pu+qvACfJjLXKG66tHlaVhAHd/1l2FocgFNUDFFuKS3mnzt9hKys7sB4aO3O0OdohN/0NJC4ldV8/OmeXqqfkiPWcgPx3C8bYyXCX7QJNBHKrzbX1jW51Px7SIDWFDV6kxGwpQGGBMJg/k79gjjM+jhn4fg1/VP/Fx37mAnfLqpcTfiOkzSE80ORGefQ1XfGK/Dpa3ITrzRYW8xlR caos-dev-arm"
pvt_key          = "~/.ssh/caos-dev-arm.cer"
otlp_endpoint    = "staging-otlp.nr-data.net:4317"
nr_license_key   = "******"
ansible_playbook = "install-requirements.yml"
