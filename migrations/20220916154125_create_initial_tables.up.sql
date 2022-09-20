CREATE TABLE uploads (
    key CHAR(7) PRIMARY KEY,
    filename VARCHAR(255) NOT NULL,
    expires TIMESTAMPTZ NOT NULL,
    date_created TIMESTAMPTZ NOT NULL DEFAULT current_timestamp
);