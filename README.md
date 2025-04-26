[![CI](https://github.com/opensourceworks-org/object-storage-proxy/actions/workflows/ci.yml/badge.svg)](https://github.com/opensourceworks-org/object-storage-proxy/actions/workflows/ci.yml)
![PyPI - Downloads](https://img.shields.io/pypi/dm/object-storage-proxy)
![PyPI - version](https://img.shields.io/pypi/v/object-storage-proxy)


# <object-storage-proxy ⚡> Yet Another Object Storage Reverse Proxy

> 📌 **Note:** This project is still under heavy development, and its APIs are subject to change.

## Introduction

A fast and safe in-process reverse proxy server, based on Cloudflare's [pingora](https://github.com/cloudflare/pingora?tab=readme-ov-file), to reverse proxy AWS and IBM Cloud Object Storage buckets and integrate your Authentication and Authorization services.

Decouples frontend from backend authentication and authorization. 


- [x] Takes a Python authorization callable (allows you to plug in your own authorization services) and api_key fetch callback function and cos bucket dictionary.
- [x] The validation is cached with optional ttl (default 5min, keep it short). 
- [x] The apikey is used to authenticate against IBM's IAM endpoint and is cached and renewed on expiration. (IBM only)
- [x] If no apikey or hmac keypair is provided, a Python function can be passed in to fetch the apikey or hmac keys for any given bucket (run once).
- [x] HMAC support: passing in access and secret id keys (or as json string from python credentials callable), will be used to sign the upstream request (AWS/IBM/..)
- [x] frontend request is signed with your own custom keypair
- [x] supports running in cloud environments / proxy headers for request signature validation

The bucket dict contains for each bucket:

    - endpoint host
    - port
    - api key (optional)
    - hmac access key (optional)
    - hmac secret key (optional)
    - ttl (optional, default 300) -> keep this reasonably short, but size to your needs

```json
cos_map = {
    "bucket1": {
        "host": "s3.eu-de.cloud-object-storage.appdomain.cloud",
        "port": 443,
        "ttl": 0
    },
    "bucket2": {
        "host": "s3.eu-de.cloud-object-storage.appdomain.cloud",
        "port": 443,
        "apikey": "apikey"
    },
    "proxy-bucket01": {
        "host": "s3.eu-de.cloud-object-storage.appdomain.cloud",
        "port": 443,
        "access_key": "<redacted>",
        "secret_key": "<redacted>",
        "ttl": 300
    },
    "proxy-aws-bucket01": {
        "host": "s3.eu-west-3.amazonaws.com",
        "region": "eu-west-3",
        "access_key": os.getenv("AWS_ACCESS_KEY"),
        "secret_key": os.getenv("AWS_SECRET_KEY"),
        "port": 443,
        "ttl": 300
    }    
}
```

The Python callables take two arguments: 

    - token: parsed from the original aws request's authorization header
    - bucket: parsed from the uri path

```python
    def your_credentials_fetcher(token: str, bucket: str) -> str
    def your_request_authorizer(token: str, bucket: str) -> bool
```

Proxy Configuration





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


![request lifecycle](https://raw.githubusercontent.com/opensourceworks-org/object-storage-proxy/62adceaddefa2ad911d80fb13a3f9cec2eff8829/img/request_lifecycle.svg)

![request stages](https://raw.githubusercontent.com/opensourceworks-org/object-storage-proxy/d8ca9ee95f820c9525fef0b703ad28a8bcceedb7/img/request_stages.svg)

# authentication & authorization
The advantage is we can plug in a python authentication function and another function for authorization, allowing for fine-grained control.

## authentication
We use the standard aws hmac header and aws v4 request signing algorithm.

## authorization
Pass in a callable from python which will be called from rust.  This will be cached (ttl) for consecutive requests.

# Examples

With local configuration.

~/.aws/config
```ini
[profile osp]
region = eu-west-3
output = json
services = osp-services
s3 =
    addressing_style = path

[services osp-services]
s3 =
  endpoint_url = http://localhost:6190
```

~/.aws/credentials
```ini
[osp]
aws_access_key_id = MYLOCAL123  # <-- this could be an openid connect/oauth2 token or anything that makes sense for your business, encode it if required
aws_secret_access_key = nothingmeaningful # <-- used for compatibility with aws sdk, to sign original request, but is ignored later
```

Set up a minimal server implementation:

```python
import json
import os
import random
import object_storage_proxy as osp

from dotenv import load_dotenv

from object_storage_proxy import start_server, ProxyServerConfig


_TRUES  = {"y", "yes", "t", "true", "on", "1"}
_FALSES = {"n", "no", "f", "false", "off", "0"}


def strtobool(val: str) -> bool:
    """Convert a string to True/False, raise ValueError otherwise."""
    v = val.lower()
    if v in _TRUES:
        return True
    if v in _FALSES:
        return False
    raise ValueError(f"invalid truth value {val!r}")


def do_api_creds(token: str, bucket: str) -> str:
    """Fetch credentials (ro, rw, access_denied) for the given bucket, depending on the token. """
    apikey = os.getenv("COS_API_KEY")
    if not apikey:
        raise ValueError("COS_API_KEY environment variable not set")
    
    print(f"Fetching credentials for {bucket}...")
    return apikey


def do_hmac_creds(token: str, bucket: str) -> str:
    """ Fetch HMAC credentials (ro, rw, access_denied) for the given bucket, depending on the token """
    access_key = os.getenv("ACCESS_KEY")
    secret_key = os.getenv("SECRET_KEY")
    if not access_key or not secret_key:
        raise ValueError("ACCESS_KEY or SECRET_KEY environment variable not set")
        
    print(f"Fetching HMAC credentials for {bucket}...")

    return json.dumps({
        "access_key": access_key,
        "secret_key": secret_key
    })

def lookup_secret_key(access_key: str) -> str | None:
    # get all environment variables ending in ACCESS_KEY
    access_keys = [{key:value} for key, value in os.environ.items() if key.endswith("ACCESS_KEY") and value==access_key ]

    if len(access_keys) > 0:
        access_key_var = next((k for k, v in access_keys[0].items() if v == access_key), None)

        secret_key_var = access_key_var.replace("ACCESS_KEY", "SECRET_KEY")
        return os.getenv(secret_key_var, None)
    else:
        print(f"no access keys found for : {access_key}")


def do_validation(token: str, bucket: str) -> bool:
    """ Authorize the request based on token for the given bucket. 
        You can plug in your own authorization service here.
        The token is the authorization token passed in the request.
        The bucket is the bucket name.
        The function should return True if the request is authorized, False otherwise.
    """

    print(f"PYTHON: Validating headers: {token} for {bucket}...")
    # return random.choice([True, False])
    return True


def main() -> None:
    load_dotenv()

    counting = strtobool(os.getenv("OSP_ENABLE_REQUEST_COUNTING", "false"))

    if counting:
        osp.enable_request_counting()
        print("Request counting enabled")

    apikey = os.getenv("COS_API_KEY")
    if not apikey:
        raise ValueError("COS_API_KEY environment variable not set")

    cos_map = {
        "bucket1": {
            "host": "s3.eu-de.cloud-object-storage.appdomain.cloud",
            "region": "eu-de",
            "port": 443,
            "apikey": apikey,
            "ttl": 0
        },
        "bucket2": {
            "host": "s3.eu-de.cloud-object-storage.appdomain.cloud",
            "region": "eu-de",
            "port": 443,
            "apikey": apikey
        },
        "proxy-bucket01": {
            "host": "s3.eu-de.cloud-object-storage.appdomain.cloud",
            "region": "eu-de",
            # "access_key": os.getenv("ACCESS_KEY"),
            # "secret_key": os.getenv("SECRET_KEY"),
            "port": 443,
            "ttl": 300
        },
        "proxy-bucket05": {
            "host": "s3.eu-de.cloud-object-storage.appdomain.cloud",
            "region": "eu-de",
            "access_key": os.getenv("PROXY_BUCKET05_ACCESS_KEY"),
            "secret_key": os.getenv("PROXY_BUCKET05_SECRET_KEY"),
            "port": 443,
            "ttl": 300
        },
        "proxy-aws-bucket01": {
            "host": "s3.eu-west-3.amazonaws.com",
            "region": "eu-west-3",
            "access_key": os.getenv("AWS_ACCESS_KEY"),
            "secret_key": os.getenv("AWS_SECRET_KEY"),
            "port": 443,
            "ttl": 300
        }
    }

    hmac_keys= [
        # {
        #     "access_key": os.getenv("LOCAL_ACCESS_KEY"),
        #     "secret_key": os.getenv("LOCAL_SECRET_KEY")
        # },
        {
            "access_key": os.getenv("LOCAL2_ACCESS_KEY"),
            "secret_key": os.getenv("LOCAL2_SECRET_KEY")
        },

    ]

    ra = ProxyServerConfig(
        cos_map=cos_map,
        bucket_creds_fetcher=do_hmac_creds,
        validator=do_validation,
        http_port=6190,
        # https_port=8443,
        threads=1,
        # verify=False,
        hmac_keystore=hmac_keys,
        skip_signature_validation=False,
        hmac_fetcher=lookup_secret_key
    )

    start_server(ra)


if __name__ == "__main__":
    main()

```

Run with [aws-cli](https://docs.aws.amazon.com/cli/latest/userguide/getting-started-install.html) (but could be anything compatible with the aws s3 api like polars, spark, presto, ...):

```bash
$ aws s3 ls s3://proxy-bucket01/ --recursive --summarize --human-readable --profile osp
2025-04-17 17:45:30   33 Bytes README.md
2025-04-17 17:48:04   33 Bytes README2.md

Total Objects: 2
   Total Size: 66 Bytes
$
```

Server output:

```log
$ uv run python test_server.py
2025-04-19T13:19:54.402023+02:00  INFO object_storage_proxy: Logger initialized; starting server on http port 6190 and https port 8443
2025-04-19T13:19:54.402361+02:00  INFO object_storage_proxy: Bucket creds fetcher provided: Py(0x100210680)
Fetching credentials for bucket01...
2025-04-19T13:19:54.402485+02:00  INFO object_storage_proxy: Callback returned: Kn2t...
[src/lib.rs:327:5] &run_args.cos_map = Py(
    0x000000010061aa00,
)
2025-04-19T13:19:54.403738+02:00  INFO pingora_core::server: Bootstrap starting
2025-04-19T13:19:54.403852+02:00  INFO pingora_core::server: Bootstrap done
2025-04-19T13:19:54.424489+02:00  INFO pingora_core::server: Server starting
PYTHON: Validating headers: MYLOCAL123 for proxy-bucket01...
2025-04-19T13:19:58.124729+02:00  INFO object_storage_proxy::utils::validator: Callback returned: false
PYTHON: Validating headers: MYLOCAL123 for proxy-bucket01...
2025-04-19T13:20:00.919320+02:00  INFO object_storage_proxy::utils::validator: Callback returned: true
2025-04-19T13:20:01.181775+02:00  INFO object_storage_proxy::credentials::secrets_proxy: No cached token found for proxy-bucket01, fetching ...
2025-04-19T13:20:01.181859+02:00  INFO object_storage_proxy::credentials::secrets_proxy: Fetching bearer token for the API key
2025-04-19T13:20:01.739385+02:00  INFO object_storage_proxy::credentials::secrets_proxy: Received access token
2025-04-19T13:20:01.739600+02:00  INFO object_storage_proxy::credentials::secrets_proxy: Fetched new token for proxy-bucket01
2025-04-19T13:20:01.739668+02:00  INFO object_storage_proxy: Sending request to upstream: https://proxy-bucket01.s3.eu-de.cloud-object-storage.appdomain.cloud/?list-type=2&prefix=&encoding-type=url
2025-04-19T13:20:01.739922+02:00  INFO object_storage_proxy: Request sent to upstream.
```

# test

See the included [python test script](https://github.com/opensourceworks-org/object-storage-proxy/blob/main/test_server.py).

Create self-signed certificates and export the environment variables:

```bash
openssl req -x509 -newkey rsa:4096 -sha256 -nodes \
        -keyout key.pem -out cert.pem -days 365 -subj "/CN=localhost"
```

```bash
export TLS_CERT_PATH=/full/path/cert.pem
export TLS_KEY_PATH=/full/path/key.pem
```

# Status

- [x] pingora proxy implementation
- [x] pass in credentials handler (which may return either api key string or json string with access_key and secret_key  )
- [x] cache credentials
- [x] pass in bucket/instance and bucket/port config
- [x] <del>split in workspace crate with core, cli and python crates</del> (too many specifics for python)
- [x] config mgmt
- [x] cache authorization (with optional ttl)
- [x] http frontend (optional)
- [x] https frontend (supports HTTP/2) (optional)
- [x] configurable request counting
- [x] call the api key fetcher callback only once, save to cos map
- [x] config for #threads in ProxyServerConfig
- [ ] also pass path and method to python callbacks and cache by token/bucket/path/method (identity based access/cache)
- [x] option to disable upstream/peer certificate validation (for development, not production!)
- [ ] expose pingora proxy server and services configuration to python
- [x] drop proxy headers (x-forwarded-proto, x-forwarded-host, ..) for signing