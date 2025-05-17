-- 2. Create the external customer table
CREATE TABLE default.customer (
  c_custkey     BIGINT,
  c_name        VARCHAR,
  c_address     VARCHAR,
  c_nationkey   BIGINT,
  c_phone       VARCHAR,
  c_acctbal     DOUBLE,
  c_mktsegment  VARCHAR,
  c_comment     VARCHAR
)
WITH (
  external_location      = 's3a://proxy-aws-bucket01/tpch/sf10/customer/',
  format                 = 'PARQUET'
);

-- 3. (Optional) Verify the data loaded
SHOW TABLES LIKE 'customer';

-- 4. Run a sample query
SELECT c_mktsegment,
       COUNT(*)       AS cnt,
       AVG(c_acctbal) AS avg_bal
FROM default.customer
WHERE c_acctbal > 0
GROUP BY c_mktsegment
ORDER BY cnt DESC
LIMIT 10;

SELECT
  c.c_custkey    AS custkey,
  c.c_name       AS name,
  COUNT(o.o_orderkey)    AS order_count,
  MAX(o.o_orderdate)     AS last_order_date
FROM default.customer AS c
JOIN default.orders            AS o
  ON c.c_custkey = o.o_custkey
GROUP BY
  c.c_custkey,
  c.c_name
ORDER BY
  order_count DESC
LIMIT 10;


trino --server localhost:8080 \
      --catalog hive \
      --schema default

SET SESSION spill_enabled = true;

WITH order_stats AS (
  SELECT
    o.o_custkey   AS custkey,
    COUNT(*)      AS order_count,
    MAX(o.o_orderdate) AS last_order_date
  FROM hive.default.orders AS o
  GROUP BY
    o.o_custkey
  ORDER BY
    order_count DESC
  LIMIT 100
)
SELECT
  os.custkey,
  c.c_name          AS name,
  os.order_count,
  os.last_order_date
FROM order_stats AS os
JOIN hive.default.customer AS c
  ON os.custkey = c.c_custkey
ORDER BY
  os.order_count DESC
LIMIT 10;
