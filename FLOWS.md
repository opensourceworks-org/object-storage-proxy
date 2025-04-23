request lifecycle sequence diagram

```text
@startuml
participant Client
participant ReverseProxy
participant IAM_Service
participant IBM_COS

Client -> ReverseProxy: Path-style Request
activate ReverseProxy

ReverseProxy -> ReverseProxy: Extract credentials from request
ReverseProxy -> ReverseProxy: Check authorization for valid token
alt Vaidation Not Found or Expired
    ReverseProxy -> Validation_Service: Request Validation
    activate Validation_Service

    Validation_Service --> ReverseProxy: Return Authorization Validation
    deactivate Validation_Service

    ReverseProxy -> ReverseProxy: Cache validation
else Validation Valid
    ReverseProxy -> ReverseProxy: Use Cached Validation
end
ReverseProxy -> ReverseProxy: Check cache for valid credentials
alt Credentials Not Found or Expired
    ReverseProxy -> IAM_Service: Request IAM Verification
    activate IAM_Service

    IAM_Service --> ReverseProxy: Return Verified Credentials
    deactivate IAM_Service

    ReverseProxy -> ReverseProxy: Cache credentials
else Credentials Valid
    ReverseProxy -> ReverseProxy: Use Cached Credentials
end

ReverseProxy -> ReverseProxy: Translate path-style to virtual-style request
ReverseProxy -> ReverseProxy: Handle secrets and endpoint (incl. port)

ReverseProxy -> IBM_COS: Forward Virtual-style Request
activate IBM_COS

IBM_COS --> ReverseProxy: Response
ReverseProxy --> Client: Return Response

deactivate IBM_COS
deactivate ReverseProxy
@enduml
```

request lifecycle
```text
@startuml
skinparam activity {
  BackgroundColor #F9F9F9
  BorderColor #333333
}

start
:Receive HTTP Request;

note left
  Context available through all stages with:
  – bucket configuration  
  – credentials cache  
  – validation/authorization cache
end note

partition request_filter {
  :request_filter();
  :Parse token from Authorization header;
  :Parse bucket from URI;
  :Invoke Python authorization callback(token, bucket);
  :Cache auth response;
}

partition upstream_peer {
  :upstream_peer();
  :Configure upstream host;
  note right
    Optional: ignore SSL cert  
    and host errors
  end note
}

partition upstream_request_filter {
  :upstream_request_filter();
  :Check bucket configuration for credentials;
  if (credentials exist?) then (yes)
    :Use configured credentials;
  else (no)
    :Invoke Python fetch_credentials callback(token, bucket)  
    --> returns api_key or hmac secrets;
    :Update bucket configuration with secrets;
  endif

  if (match secret type "api_key" (IBM) or HMAC (AWS/IBM/...) ) then (api_key)
    :POST /v1/authenticate to IBM IAM;
    :Cache bearer token (TTL);
    :Add "Authorization: Bearer <token>" header;
  else (hmac)
    :Perform AWS4-HMAC-SHA256 signing;
    :Add "x-amz-date" header;
    :Add "x-amz-content-sha256" header;
    :Add AWS "Authorization" header;
  endif
}

:Forward filtered & signed request;
stop
@enduml
```
