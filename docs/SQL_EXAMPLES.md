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

## CSV import

Header row names the columns. Extra table columns that are nullable become `NULL`.

```sql
COPY users FROM 'users.csv';
```

In the REPL:

```text
.import users.csv users
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

