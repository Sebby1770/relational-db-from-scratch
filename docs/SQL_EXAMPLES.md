# SQL Examples

This file describes the SQL surface the database should eventually support. Do not implement all of this at once. Add syntax only when the storage and execution engine are ready.

## Milestone 1-3: Basic CRUD

```sql
CREATE TABLE users (
  id INT,
  name TEXT,
  active BOOL
);

INSERT INTO users VALUES (1, 'Ada Lovelace', true);
INSERT INTO users VALUES (2, 'Grace Hopper', false);

SELECT * FROM users;
SELECT id, name FROM users WHERE active = true;

UPDATE users SET active = true WHERE id = 2;

DELETE FROM users WHERE id = 1;
```

## Constraints and Indexes

```sql
CREATE TABLE users (
  id INT PRIMARY KEY,
  email TEXT UNIQUE NOT NULL,
  name TEXT NOT NULL,
  active BOOL
);

CREATE INDEX users_email_idx ON users(email);
CREATE INDEX users_active_idx ON users(active);

EXPLAIN SELECT id FROM users WHERE email = 'ada@example.com';
```

## Expressions

```sql
SELECT id, name
FROM users
WHERE active = true AND id >= 10;

UPDATE users
SET name = 'Ada Byron'
WHERE id = 1;
```

## Joins

```sql
CREATE TABLE orders (
  id INT PRIMARY KEY,
  user_id INT,
  total INT
);

SELECT users.name, orders.total
FROM users
JOIN orders ON users.id = orders.user_id
WHERE orders.total > 100;
```

## Aggregation

```sql
SELECT active, COUNT(*)
FROM users
GROUP BY active;

SELECT user_id, SUM(total)
FROM orders
GROUP BY user_id
HAVING SUM(total) > 500;

SELECT DISTINCT user_id FROM orders;
SELECT MIN(total), MAX(total), AVG(total) FROM orders;
SELECT COUNT(email) FROM users;
SELECT name, CASE WHEN active = true THEN 'yes' ELSE 'no' END FROM users;
```

## Sorting and Limits

```sql
SELECT id, name
FROM users
WHERE active = true
ORDER BY name ASC
LIMIT 10 OFFSET 0;

SELECT name FROM users WHERE name LIKE 'Ada%';
SELECT name FROM users WHERE name LIKE '%Hopper';
SELECT id FROM users WHERE id BETWEEN 1 AND 10;
SELECT name FROM users WHERE id IN (1, 2);
SELECT id FROM users WHERE id NOT BETWEEN 1 AND 10;
SELECT name FROM users WHERE id NOT IN (1, 2);
SELECT id FROM users WHERE email IS NULL;
SELECT id FROM users WHERE email IS NOT NULL;
```

## INSERT SELECT

```sql
INSERT INTO archived
SELECT id, email, name, active
FROM users
WHERE active = false;

INSERT INTO order_names
SELECT users.name, orders.total
FROM users
JOIN orders ON users.id = orders.user_id
WHERE orders.total > 100;
```

## UNION / EXCEPT / INTERSECT

```sql
SELECT id FROM users
UNION
SELECT id FROM archived
ORDER BY id;

SELECT name FROM users
UNION ALL
SELECT name FROM archived;

SELECT id FROM users
EXCEPT
SELECT id FROM archived;

SELECT id FROM users
INTERSECT
SELECT id FROM archived;
```

## CREATE TABLE AS / TRUNCATE / RENAME

```sql
CREATE TABLE IF NOT EXISTS active_users AS
SELECT id, name FROM users WHERE active = true;

ALTER TABLE active_users RENAME TO current_users;
TRUNCATE TABLE current_users;
DROP TABLE IF EXISTS missing;
```

## ALTER TABLE

```sql
ALTER TABLE users ADD COLUMN nickname TEXT;
SELECT id, COALESCE(nickname, name) FROM users ORDER BY 1;
```

Existing rows receive `NULL` in the new column.

## CSV import and export

Header row names the columns. Extra table columns that are nullable become `NULL`. Export writes the header plus every row.

```sql
COPY users FROM 'users.csv';
COPY users TO 'users-out.csv';
```

In the REPL:

```text
.import users.csv users
.export users-out.csv users
```

## Transactions

```sql
BEGIN;
INSERT INTO users VALUES (3, 'Katherine Johnson', true);
UPDATE users SET active = false WHERE id = 2;
COMMIT;

BEGIN;
DELETE FROM users WHERE id = 3;
ROLLBACK;
```

## Durability and Admin Commands

```sql
ANALYZE users;

EXPLAIN SELECT *
FROM users
WHERE email = 'ada@example.com';

CHECKPOINT;
VACUUM;
```

## Far-Future Stretch SQL

```sql
CREATE TABLE events (
  id INT PRIMARY KEY,
  payload TEXT,
  created_at INT
);

CREATE INDEX events_created_at_idx ON events(created_at);

SELECT *
FROM events
WHERE created_at >= 1000 AND created_at <= 2000
ORDER BY created_at;
```

