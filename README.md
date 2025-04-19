[![CI](https://github.com/opensourceworks-org/object-storage-proxy/actions/workflows/ci.yml/badge.svg)](https://github.com/opensourceworks-org/object-storage-proxy/actions/workflows/ci.yml)

# ** <object-storage-proxy ⚡> Yet Another Object Storage Reverse Proxy**

> 📌 **Note:** This project is still under heavy development, and its APIs are subject to change.

## Introduction

### secrets
IBM COS Storage is built in a way where buckets are grouped by a cos (Cloud Object Storage) instance.  Access to a bucket is managed by either an api key or hmac secrets, configured on the cos instance.  

### endpoint
Each bucket has its own endpoint: <bucket_name>.s3.<region>.cloud-object-storage.appdomain.cloud:<port>.

The port is not always different, though, but it might be.  Depends on your implementation.

You can imagine managing multiple buckets across instances can become quite cumbersome, even with aws profiles etc.


### solution
There are two ways to access a bucket: through virtual addressing style (bucket.ibm-cos-host:port) and path style (ibm-cos-host/bucket).

your client (aws s3 compatible) -> http(s)://this-proxy/bucket01 -> https://bucket01.s3.eu-de.cloud-object-storage.appdomain.cloud:443

1) translate path style to virtual style
2) abstract authentication & authorization


Pass in a function which maps bucket to instance (credentials), and a function to map bucket to port (endpoint)


```text
     ┌──────┐           ┌────────────┐                                              ┌───────────┐          ┌───────┐
     │Client│           │ReverseProxy│                                              │IAM_Service│          │IBM_COS│
     └───┬──┘           └──────┬─────┘                                              └─────┬─────┘          └───┬───┘
         │Path-style Request  ┌┴┐                                                         │                    │    
         │──────────────────> │ │                                                         │                    │    
         │                    │ │                                                         │                    │    
         │                    │ │ ────┐                                                   │                    │    
         │                    │ │     │ Extract credentials from request                  │                    │    
         │                    │ │ <───┘                                                   │                    │    
         │                    │ │                                                         │                    │    
         │                    │ │ ────┐                                                   │                    │    
         │                    │ │     │ Check cache for valid credentials                 │                    │    
         │                    │ │ <───┘                                                   │                    │    
         │                    │ │                                                         │                    │    
         │                    │ │                                                         │                    │    
         │    ╔══════╤════════╪═╪═════════════════════════════════════════════════════════╪═══════════════╗    │    
         │    ║ ALT  │  Credentials Not Found or Expired                                  │               ║    │    
         │    ╟──────┘        │ │                                                         │               ║    │    
         │    ║               │ │                Request IAM Verification                ┌┴┐              ║    │    
         │    ║               │ │ ──────────────────────────────────────────────────────>│ │              ║    │    
         │    ║               │ │                                                        └┬┘              ║    │    
         │    ║               │ │               Return Verified Credentials               │               ║    │    
         │    ║               │ │ <─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─│               ║    │    
         │    ║               │ │                                                         │               ║    │    
         │    ║               │ │ ────┐                                                   │               ║    │    
         │    ║               │ │     │ Cache credentials                                 │               ║    │    
         │    ║               │ │ <───┘                                                   │               ║    │    
         │    ╠═══════════════╪═╪═════════════════════════════════════════════════════════╪═══════════════╣    │    
         │    ║ [Credentials Valid]                                                       │               ║    │    
         │    ║               │ │ ────┐                                                   │               ║    │    
         │    ║               │ │     │ Use Cached Credentials                            │               ║    │    
         │    ║               │ │ <───┘                                                   │               ║    │    
         │    ╚═══════════════╪═╪═════════════════════════════════════════════════════════╪═══════════════╝    │    
         │                    │ │                                                         │                    │    
         │                    │ │ ────┐                                                   │                    │    
         │                    │ │     │ Translate path-style to virtual-style request     │                    │    
         │                    │ │ <───┘                                                   │                    │    
         │                    │ │                                                         │                    │    
         │                    │ │ ────┐                                                   │                    │    
         │                    │ │     │ Handle secrets and endpoint (incl. port)          │                    │    
         │                    │ │ <───┘                                                   │                    │    
         │                    │ │                                                         │                    │    
         │                    │ │                        Forward Virtual-style Request    │                   ┌┴┐   
         │                    │ │ ───────────────────────────────────────────────────────────────────────────>│ │   
         │                    │ │                                                         │                   │ │   
         │                    │ │                                  Response               │                   │ │   
         │                    │ │ <─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─ ─│ │   
         │                    └┬┘                                                         │                   └┬┘   
         │  Return Response    │                                                          │                    │    
         │<─ ─ ─ ─ ─ ─ ─ ─ ─ ─ │                                                          │                    │    
     ┌───┴──┐           ┌──────┴─────┐                                              ┌─────┴─────┐          ┌───┴───┐
     │Client│           │ReverseProxy│                                              │IAM_Service│          │IBM_COS│
     └──────┘           └────────────┘                                              └───────────┘          └───────┘
```

# authentication & authorization
The advantage is we can plug in a python authentication function and another function for authorization, allowing for fine-grained control.

## authentication
We use the standard aws hmac header.

## authorization
Pass in a callable from python which will be called from rust.  This will be cached (ttl) for consequtive requests.

# Examples

With local configuration.

~/.aws/config
```
[profile osp]
region = eu-west-3
output = json
services = pingora-services
s3 =
    addressing_style = path

[services osp-services]
s3 =
  endpoint_url = http://localhost:6190
```

~/.aws/credentials
```
[osp]
aws_access_key_id = MYLOCAL123
aws_secret_access_key = nothingmeaningful
```

Set up a minimal server implementation:

```python
from object_storage_proxy import start_server, ProxyServerConfig
from dotenv import load_dotenv
import os
import random


load_dotenv()


def docreds(bucket):
    apikey = os.getenv("COS_API_KEY")
    if not apikey:
        raise ValueError("COS_API_KEY environment variable not set")
    
    print(f"Fetching credentials for {bucket}...")
    return apikey

def do_validation(token: str, bucket: str) -> bool:
    print(f"PYTHON: Validating headers: {token} for {bucket}...")
    return random.choice([True, False])


def main():

    apikey = os.getenv("COS_API_KEY")
    if not apikey:
        raise ValueError("COS_API_KEY environment variable not set")


    cos_mapping = [
        ("bucket1", "s3.eu-de.cloud-object-storage.appdomain.cloud", 443, "instance1", apikey),
        ("bucket2", "s3.eu-de.cloud-object-storage.appdomain.cloud", 443, "instance2", apikey),
        ("proxy-bucket01", "s3.eu-de.cloud-object-storage.appdomain.cloud", 443, "instance3", apikey),
    ]


    ra = ProxyServerConfig(
        bucket_creds_fetcher=docreds,
        validator=do_validation,
        cos_map=cos_mapping,
        port=6190
    )

    start_server(ra)


if __name__ == "__main__":
    main()

```

run with [aws-cli]() (but could be anything compatible with the aws s3 api like polars, spark, presto, ...):

```shell
$ aws s3 ls s3://proxy-bucket01/ --recursive --summarize --human-readable --profile osp
2025-04-17 17:45:30   33 Bytes README.md
2025-04-17 17:48:04   33 Bytes README2.md

Total Objects: 2
   Total Size: 66 Bytes
$
```


# Status

- [x] pingora proxy implementation
- [x] pass in credentials handler
- [x] cache credentials
- [x] pass in bucket/instance and bucket/port config
- [x] <del>split in workspace crate with core, cli and python crates</del> (too many specifics for python)
- [ ] config mgmt
- [ ] cache authorization (with optional ttl)