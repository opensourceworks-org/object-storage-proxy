# **Yet Another Object Storage Reverse Proxy**

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

# Status

- [x] pingora proxy implementation
- [x] pass in credentials handler
- [x] cache credentials
- [x] pass in bucket/instance and bucket/port config
- [x] <del>split in workspace crate with core, cli and python crates</del> (too many specifics for python)
- [ ] config mgmt
- [ ] cache authorization (with optional ttl)