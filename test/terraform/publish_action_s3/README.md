## Context

This TF is responsible for creating a S3 bucket named
`agent-control-publish-action-testing` in agent-control AWS account.

This bucket is intended to be used to test the publish-action.

This is not the best place for it, but as we are using the agent-control account for it
and we will be the owners of the action, this is the best place for now.

## Summary
The TF will:
* create a non-public S3 bucket
* create IAM policies so this bucket can be accessed from our VPC
* create IAM role for EC2 instances to be able to read/write into this S3.
* create instance profile to be attached to ec2 instance `ec2_s3_instance_profile`

So if you want to test the `publish action` from an EC2 (currently, the publish action runs only in amd64),
you need to attach the instance profile `ec2_s3_instance_profile` to the EC2.

Then you can check out the project, and execute it from the ec2.

