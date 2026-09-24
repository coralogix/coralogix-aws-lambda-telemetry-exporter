# Change Log

## [1.3.0] - future-release - layer # - python layer # - nodejs layer # - java layer #

Exporter:

* Add separate FIPS and non-FIPS builds. Use `aws-lc` crypto provider (previously used `ring`)
* Update libraries used by coralogix-aws-lambda-telemetry-exporter
* Drop support for Amazon Linux 1

## [#] - 2026-09-07 - layer # - python layer # - nodejs layer 42 - java layer #

NodeJS:

* Update upstream OTel libraries:
  * `instrumentation-dns` `v0.65.0`, `instrumentation-express` `v0.70.0`, `instrumentation-graphql` `v0.70.0`, `instrumentation-grpc` `v0.222.0`, `instrumentation-hapi` `v0.68.0`, `instrumentation-http` `v0.222.0`, `instrumentation-ioredis` `v0.70.0`, `instrumentation-koa` `v0.70.0`, `instrumentation-mongodb` `v0.75.0`, `instrumentation-mysql` `v0.68.0`, `instrumentation-net` `v0.66.0`, `instrumentation-pg` `v0.74.0`, `instrumentation-redis` `v0.70.0`, `instrumentation-aws-sdk` `v0.77.0`
* Drop support for NodeJS 20
* Add support for NodeJS 26

## [#] - 2026-07-03 - layer # - python layer # - nodejs layer 37 - java layer #

NodeJS:

* Update upstream OTel libraries:
  * `instrumentation-dns` `v0.59.0`, `instrumentation-express` `v0.64.0`, `instrumentation-graphql` `v0.64.0`, `instrumentation-grpc` `v0.216.0`, `instrumentation-hapi` `v0.62.0`, `instrumentation-http` `v0.216.0`, `instrumentation-ioredis` `v0.64.0`, `instrumentation-koa` `v0.64.0`, `instrumentation-mongodb` `v0.73.0`, `instrumentation-mysql` `v0.62.0`, `instrumentation-net` `v0.60.0`, `instrumentation-pg` `v0.68.0`, `instrumentation-redis` `v0.64.0`, `instrumentation-aws-sdk` `v0.75.0`

## [#] - 2026-05-26 - layer # - python layer 32 - nodejs layer 36 - java layer 18

* Update upstream libraries for Python, Java and NodeJS:
  * Python `v0.63.0`: instrumentation-aiohttp-client, util-http, instrumentation-asgi, instrumentation-boto, instrumentation-asyncpg, instrumentation-celery, instrumentation-dbapi, instrumentation-django, instrumentation-elasticsearch, instrumentation-fastapi, instrumentation-falcon, instrumentation-flask, instrumentation-grpc, instrumentation-jinja2, instrumentation-mysql, instrumentation-psycopg2, instrumentation-pymemcache, instrumentation-pymongo, instrumentation-pymysql, instrumentation-pyramid, instrumentation-redis, instrumentation-requests, instrumentation-sqlalchemy, instrumentation-sqlite3, instrumentation-starlette, instrumentation-tornado, instrumentation-wsgi
  * NodeJS: instrumentation-dns `v0.59.0`, instrumentation-express `v0.64.0`, instrumentation-graphql `v0.64.0`, instrumentation-grpc `v0.216.0`, instrumentation-hapi `v0.62.0`, instrumentation-http `v0.216.0`, instrumentation-ioredis `v0.64.0`, instrumentation-koa `v0.64.0`, instrumentation-mongodb `v0.73.0`, instrumentation-mysql `v0.62.0`, instrumentation-net `v0.60.0`, instrumentation-pg `v0.68.0`, instrumentation-redis `v0.64.0`, instrumentation-aws-sdk `v0.75.0`
  * Java SDK version `2.28.0`: aws-lambda-events, aws-sdk
  * Java: add support for triggers DynamoDB, Cognito, EventBridge, SNS, Kinesis, Step Functions
  * NodeJS: add support for SNS and Kinesis triggers
* Inspect and patch sdk libraries to ensure they work in FIPS environments.

## [#] - 2025-12-17 - layer # - python layer # - nodejs layer 31 - java layer #

Exporter:

* Add separate FIPS and non-FIPS builds. Use `aws-lc` crypto provider (previously used `ring`)
* Update libraries used by coralogix-aws-lambda-telemetry-exporter
* Drop support for Amazon Linux 1

## [#] - 2025-12-17 - layer # - python layer # - nodejs layer 31 - java layer #

NodeJS:

* Update upstream OTel libraries:
  * `instrumentation-dns` `v0.43.1`, `instrumentation-express` `v0.47.1`, `instrumentation-graphql` `v0.47.1`, `instrumentation-grpc` `v0.57.2`, `instrumentation-hapi` `v0.45.1`, `instrumentation-http` `v0.57.2`, `instrumentation-ioredis` `v0.47.1`, `instrumentation-koa` `v0.47.1`, `instrumentation-mongodb` `v0.61.0`, `instrumentation-mysql` `v0.45.1`, `instrumentation-net` `v0.43.1`, `instrumentation-pg` `v0.51.1`, `instrumentation-redis` `v0.46.1`, `instrumentation-aws-sdk` `v0.64.0`
* Add support for NodeJS 24

## [1.2.1] - 2025-11-28 - layer 38 - python layer 29 - nodejs layer 30 - java layer 17

Exporter:

* Update libraries used by coralogix-aws-lambda-telemetry-exporter

NodeJS:

* Update upstream OTel libraries:
  * `instrumentation-dns` `v0.43.1`, `instrumentation-express` `v0.47.1`, `instrumentation-graphql` `v0.47.1`, `instrumentation-grpc` `v0.57.2`, `instrumentation-hapi` `v0.45.1`, `instrumentation-http` `v0.57.2`, `instrumentation-ioredis` `v0.47.1`, `instrumentation-koa` `v0.47.1`, `instrumentation-mongodb` `v0.61.0`, `instrumentation-mysql` `v0.45.1`, `instrumentation-net` `v0.43.1`, `instrumentation-pg` `v0.51.1`, `instrumentation-redis` `v0.46.1`, `instrumentation-aws-sdk` `v0.64.0`
* Optimise layer size

## [#] - 2025-08-20 - layer # - python layer # - nodejs layer 29 - java layer #

NodeJS:

* Update upstream OTel libraries:
  * `instrumentation-dns` `v0.43.1`, `instrumentation-express` `v0.47.1`, `instrumentation-graphql` `v0.47.1`, `instrumentation-grpc` `v0.57.2`, `instrumentation-hapi` `v0.45.1`, `instrumentation-http` `v0.57.2`, `instrumentation-ioredis` `v0.47.1`, `instrumentation-koa` `v0.47.1`, `instrumentation-mongodb` `v0.52.0`, `instrumentation-mysql` `v0.45.1`, `instrumentation-net` `v0.43.1`, `instrumentation-pg` `v0.51.1`, `instrumentation-redis` `v0.46.1`, `instrumentation-aws-sdk` `v0.49.1`
* Add support for Step Functions

## [#] - 2025-05-19 - layer # - python layer 26 - nodejs layer # - java layer #

* Fix unhandled exception when `PutObject` is called with non-ascii payload

## [1.2.0] - 2025-04-29 - layer 37 - python layer 25 - nodejs layer 28 - java layer 16

Exporter:

* Add new way of sending telemetry with logs, traces and metrics sent in one request (`CX_COMBINED_TELEMETRY_ENABLED=true`)
* Limit the errors for which a retry of sending telemetry is attempted

NodeJS:

* Fix support for SNS trigger span links

Python:

* Fix broken SQS trigger span links

## [1.1.0] - 2025-04-07 - layer 36 - python layer 24 - nodejs layer 27 - java layer 15

Exporter:

* Enable fine-tunning of OTel resource attributes and cx_metadata with:
  * `CX_LOGS_METADATA_INCLUDE_TRACE_REF` - Toggles presence of `cx_metadata.span_id` and `cx_metadata.trace_id` in logs
  * `CX_LOGS_METADATA_INCLUDE_EXECUTION` - Toggles presence of `cx_metadata.execution` in logs
  * `CX_LOGS_METADATA_INCLUDE_INVOCATION_ID` - Toggles presence of `cx_metadata.invocation_id` in logs (an alternative for `cx_metadata.execution`).
  * `CX_*RESOURCE_BUILT_IN_ATTRIBUTES` - Controls which built-in attributes should be included in resource attributes in `cx_metadata`
  * `CX_*RESOURCE_EXTRA_ATTRIBUTES` - Add extra, user-defined attributes
* Enable trimming down platform event logs with:
  * `CX_PLATFORM_LOGS_INCLUDE_REQUEST_ID` - Toggles presence of `request_id` in the event body. (Users may want to disable as it is also present as `cx_metadata.execution_id` )
  * `CX_PLATFORM_LOGS_HIDE_DEFAULT_VALUES` - The event logs contain some fields that are only populated in rare cases, like failures or cold starts, and so in most logs they carry a `null` or other default value. This config property can be used to hide these fields unless they contain any valuable information.
* Rename `CX_LOG_METADATA_ENABLED` to `CX_LOGS_METADATA_ENABLED` (while keeping support for the old name)
* Enable sending part of telemetry to OTel URL while the rest is sent to firehose
* Drop Python 3.8 and Dotnet 6 from the list of supported runtimes in order to fit in the limit of 15. lambda-telemetry-exporter will continue to work with these runtimes.
* Add nodejs 22 and Python 3.13 support

Python:

* Avoid crash when a list is provided to botocore instrumentation

## [#] - 2025-03-25 - layer # - python layer 23 - nodejs layer 26 - java layer 14

Python:

* Add Python 3.13 as supported runtime.
* Update instrumentation packages to version `v0.52.0b`.

NodeJS:

* Update  core instrumentation to version `v1.30.5`
* Updated Lambda instrumentation package version to `v0.50.3`. Notable changes:
  * Cold starts are identified and reported in the attribute `faas.coldstart`.

Java:

* Update instrumentation packages to version `v.1.33.6`.

## [1.0.1] - 2025-02-05 - layer 35 - python layer 20 - nodejs layer 25 - java layer 13

* Make firehose errors more verbose

## [1.0.0] - 2025-01-20 - layer 34 - python layer 19 - nodejs layer 24 - java layer 12

* Ensure that OTLP server sockets are open before the extension is registered with AWS

## [0.9.2] - 2024-12-13 - layer 33 - python layer 18 - nodejs layer 23 - java layer 11

* Introduce `CX_PLATFORM_LOGS` which can be used to selectively disable platform event logs

## [0.9.1] - 2024-12-05 - layer 32 - python layer 17 - nodejs layer 22 - java layer 10

* Enable warming up of OTEL exporters with special spans

## [0.9.0] - 2024-11-06 - layer 31 - python layer 16 - nodejs layer 21 - java layer 9

* Fixed `GLIBC` issue with arm64 layer.
* ALPN is disabled by default (HTTP2 support is assumed) when sending telemetry to Coralogix or CX_OTEL_URL. It can be reenabled with `CX_OTEL_APLN_ENABLED=t`.
* Fixed issue with handling spans received after runtime_done event
* Upgrade to hyper v1 and tonic v0.12 libraries
* The layers are now also published in a new AWS region `ap-southeast-5`

## [0.8.0] - 2024-10-14 - layer 30 - python layer 15 - nodejs layer 19 - java layer 8

* Add detection of OOM based on "Runtime.OutOfMemory" error_type
* Add Add `CX_OTEL_LOGS_URL`, `CX_OTEL_TRACES_URL`, `CX_OTEL_METRICS_URL`. This adds a way to send one pillar of telemetry to a different destination (for example for processing in central otel-collector) while sending the rest directly to Coralogix

## [0.7.0] - 2024-08-02 - layer 29 - python layer 14 - nodejs layer 14 - java layer 7

* Introduced "Early spans" feature, which creates meaningful trigger and invocation spans in case of timeout or other function crash (init failures not included), that normally prevents delivery of the spans from OTEL instrumentation.
* Introduced processing of OTEL metrics to reduce the volume of metrics sent to Coralogix, by including only the data that is a meaningful update. This can be disabled with `CX_OTEL_METRICS_MODE=direct`
* Fix OOM detection in case of memory usage higher than the limit

## [0.6.5] - 2024-06-29 - layer 28 - python layer 13 - nodejs layer 12 - java layer 6

* Fixed issue with spans with `kind=consumer` not being recognized as the main function span

## [0.6.4] - 2024-05-27 - layer 27 - python layer 12 - nodejs layer 11 - java layer 5

* Added CX_SERVICE_NAME which can be used to customize the service name (the default is the function's name)

## [0.6.3] - 2024-03-29 - layer 26 - python/nodejs layer 8 - java layer 3

* Fixed issue with `CX_TRACING_MODE=disabled` and `CX_TRACING_MODE=telemetry_api` propagating OTEL traces to coralogix (only `CX_TRACING_MODE=otel` should do that)

## [0.6.2] - 2023-11-30 - layer 25 - python/nodejs layer 7 - java layer 2

* Fixed issue with truncating logs containing multi-byte UTF-8 characters

## [0.6.1] - 2023-11-27 - layer 24 - python/nodejs layer 6 - java layer 1

* Renamed `CX_PRIVATE_KEY` to `CX_API_KEY`. The old name remains supported.
* Fixed an incorrect warning about missing spans

## [0.6.0] - 2023-10-25 - layer 23 - python/nodejs layer 5

* Added possibility to load Coralogix API Key from AWS Secrets Manager (`CX_SECRET`)
* The algorithm of sending telemetry to coralogix has been redesigned to improve reliability of telemetry delivery on shutdown (including lambda timeouts).

## [0.5.2] - 2023-10-05 - layer 22 - python/nodejs layer 4

* `coralogix-aws-lambda-telemetry-exporter` can filter span attributes to remove vulnerable data like authentication headers. (`CX_EXCLUDED_SPAN_ATTRIBUTES`)
* Users no longer need to set `CX_TRACING_MODE=otel` when using `*-wrapper-and-exporter-*` layer. The `coralogix-aws-lambda-telemetry-exporter` will automatically detect that OTEL instrumentation is present and choose that mode.
* `coralogix-aws-lambda-telemetry-exporter` will send function spans (spans coming from OTEL instrumentation) early to coralogix when there's a lot of them. This is important for functions that produce 10s of thousands of spans per invocation.
* Improved behaviour when under heavy workload
* Improved behaviour during shutdown of lambda, including extended debug logging
* Fixed an issue where logs produced by background threads of the function after the handler has completed wouldn't be processed and delivered to coralogix.

## [0.5.1] - 2023-09-13 - layer 21 - python/nodejs layer 3

* Added `out_of_memory` field to the report log
* Improved debug logs emitted when function instrumentation doesn't produce spans as expected
* Improved parsing of XRay IDs
* propagate `telemetry.sdk.language` attribute (results in better span icons in coralogix UI)

## [0.5.0] - 2023-08-11 - layer 20 - python/nodejs layer 2

* Experimental: Added ability to send telemetry to firehose.
* Added config options to selectively disable logs/traces/metric
* `coralogix-aws-lambda-telemetry-exporter`` will not crash the lambda function in case of a misconfiguration, and will try not to crash it in case of other fatal failures
* Fixed function handler failure detection
* Improved startup performance by avoiding initialisation of aws sdk when the related features (tags / firehose) are not used
* Updated OTLP to version 1.0.0
* Improved debug logs
* Improved creation of an initialisation span which will now be correctly created in case of function init failure and otel instrumentation failure.
* Improved reporting of metrics in case of fatal failures of lambda runtime
* Improved handling of `application_name` / `subsystem_name` in case of function initialisation failure

## [0.4.4] - 2023-07-31 - layer 18

* Fixed parsing of gov cloud ARNs
