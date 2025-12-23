-- Add 100 credits to the default team
INSERT INTO credits_transactions (id, team_id, amount, transaction_type, description, reference_id, created_at)
VALUES (
    gen_random_uuid(),
    '00000000-0000-0000-0000-000000000000'::uuid,
    100,
    'manual_adjustment',
    'Admin: Adding credits for testing',
    NULL,
    NOW()
);

-- Update the credits balance
UPDATE credits 
SET balance = balance + 100, updated_at = NOW()
WHERE team_id = '00000000-0000-0000-0000-000000000000'::uuid;

-- Verify the update
SELECT * FROM credits WHERE team_id = '00000000-0000-0000-0000-000000000000'::uuid;