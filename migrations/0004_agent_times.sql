-- Daily times ("HH:MM", the server's local time) as an alternative to a repeat interval.
ALTER TABLE agents ADD COLUMN times TEXT NOT NULL DEFAULT '[]';
