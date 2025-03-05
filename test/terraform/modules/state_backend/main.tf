resource "aws_s3_bucket" "tfstates" {
  bucket              = var.bucket_name
  object_lock_enabled = true
  # force_destroy     = true # Allow to destroy all the versions automatically created.
}

resource "aws_s3_bucket_versioning" "tfstates" {
  bucket = aws_s3_bucket.tfstates.bucket

  versioning_configuration {
    status = "Enabled"
  }
}

resource "aws_s3_bucket_ownership_controls" "tfstates" {
  bucket = aws_s3_bucket.tfstates.id
  rule {
    object_ownership = "BucketOwnerPreferred"
  }
}

resource "aws_s3_bucket_acl" "tfstates" {
  depends_on = [aws_s3_bucket_ownership_controls.tfstates]

  bucket = aws_s3_bucket.tfstates.bucket
  acl    = "private"
}

resource "aws_s3_bucket_public_access_block" "tfstates" {
  bucket = aws_s3_bucket.tfstates.id

  block_public_acls   = true
  block_public_policy = true
}

resource "aws_s3_bucket_object_lock_configuration" "tfstates" {
  bucket = aws_s3_bucket.tfstates.bucket

  rule {
    default_retention {
      mode = "COMPLIANCE"
      days = 10
    }
  }
}

resource "aws_s3_bucket_server_side_encryption_configuration" "tfstates" {
  bucket = aws_s3_bucket.tfstates.bucket

  rule {
    apply_server_side_encryption_by_default {
      sse_algorithm = "AES256"
    }
  }
}

resource "aws_dynamodb_table" "tfstates" {
  name = var.dynamodb_table_name

  read_capacity  = 5
  write_capacity = 5
  hash_key       = "LockID" # Hardcoded: https://www.terraform.io/docs/language/settings/backends/s3.html#dynamodb_table

  attribute {
    name = "LockID"
    type = "S"
  }
}
