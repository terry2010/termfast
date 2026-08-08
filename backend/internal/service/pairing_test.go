package service

import (
	"testing"

	"github.com/termfast/backend/internal/model"
)

func TestPairingInitiateAndComplete(t *testing.T) {
	db := testDB(t)
	auth := NewAuthService(db, testCfg())
	pairing := NewPairingService(db, testCfg())

	user, _ := auth.Register("test@example.com", "password123")

	// Initiate
	p, err := pairing.Initiate(user.ID, "desktop-001", "TestDesk", nil, "")
	if err != nil {
		t.Fatalf("initiate failed: %v", err)
	}
	if p.PairingID == "" {
		t.Error("expected non-empty pairing_id")
	}
	if p.Status != "pending" {
		t.Errorf("expected status pending, got %s", p.Status)
	}

	// Complete
	jwt, refresh, err := pairing.Complete(user.ID, p.PairingID, "phone-pubkey-hex", "mobile-001", "TestPhone")
	if err != nil {
		t.Fatalf("complete failed: %v", err)
	}
	if jwt == "" || refresh == "" {
		t.Error("expected non-empty tokens")
	}

	// Verify status
	status, err := pairing.Status(p.PairingID)
	if err != nil {
		t.Fatalf("status failed: %v", err)
	}
	if status.Status != "completed" {
		t.Errorf("expected status completed, got %s", status.Status)
	}
	if status.MobilePubkey != "phone-pubkey-hex" {
		t.Errorf("expected mobile pubkey, got %s", status.MobilePubkey)
	}

	// Complete again should fail
	_, _, err = pairing.Complete(user.ID, p.PairingID, "key2", "dev2", "TestPhone")
	if err == nil {
		t.Error("expected error for double complete")
	}
}

func TestPairingRevoke(t *testing.T) {
	db := testDB(t)
	auth := NewAuthService(db, testCfg())
	pairing := NewPairingService(db, testCfg())

	user, _ := auth.Register("test@example.com", "password123")
	p, _ := pairing.Initiate(user.ID, "desktop-001", "TestDesk", nil, "")
	pairing.Complete(user.ID, p.PairingID, "pubkey", "mobile-001", "TestPhone")

	// Revoke by correct user
	err := pairing.Revoke(p.PairingID, user.ID)
	if err != nil {
		t.Fatalf("revoke failed: %v", err)
	}
	status, _ := pairing.Status(p.PairingID)
	if status.Status != "revoked" {
		t.Errorf("expected revoked, got %s", status.Status)
	}

	// Refresh should fail after revoke
	_, err = pairing.RefreshPairingJWT(p.PairingID)
	if err == nil {
		t.Error("expected error refreshing revoked pairing")
	}
}

func TestPairingRevokeWrongUser(t *testing.T) {
	db := testDB(t)
	auth := NewAuthService(db, testCfg())
	pairing := NewPairingService(db, testCfg())

	user1, _ := auth.Register("user1@example.com", "password123")
	user2, _ := auth.Register("user2@example.com", "password123")
	p, _ := pairing.Initiate(user1.ID, "desktop-001", "TestDesk", nil, "")

	// User2 should not be able to revoke user1's pairing
	err := pairing.Revoke(p.PairingID, user2.ID)
	if err == nil {
		t.Error("expected error for wrong user revoke")
	}
}

func TestConfigSync(t *testing.T) {
	db := testDB(t)
	auth := NewAuthService(db, testCfg())
	pairing := NewPairingService(db, testCfg())

	user, _ := auth.Register("test@example.com", "password123")
	p, _ := pairing.Initiate(user.ID, "desktop-001", "TestDesk", nil, "")
	pairing.Complete(user.ID, p.PairingID, "pubkey", "mobile-001", "TestPhone")

	// Upload config
	err := pairing.UploadConfig(p.PairingID, "encrypted-base64-data", "nonce-base64")
	if err != nil {
		t.Fatalf("upload failed: %v", err)
	}

	// Download config
	cc, err := pairing.DownloadConfig(p.PairingID)
	if err != nil {
		t.Fatalf("download failed: %v", err)
	}
	if cc.Ciphertext != "encrypted-base64-data" {
		t.Errorf("expected ciphertext, got %s", cc.Ciphertext)
	}
	if cc.Nonce != "nonce-base64" {
		t.Errorf("expected nonce, got %s", cc.Nonce)
	}

	// Upload again should update
	pairing.UploadConfig(p.PairingID, "new-data", "new-nonce")
	cc, _ = pairing.DownloadConfig(p.PairingID)
	if cc.Ciphertext != "new-data" {
		t.Errorf("expected updated ciphertext, got %s", cc.Ciphertext)
	}
}

func TestConfigSyncNotCompleted(t *testing.T) {
	db := testDB(t)
	auth := NewAuthService(db, testCfg())
	pairing := NewPairingService(db, testCfg())

	user, _ := auth.Register("test@example.com", "password123")
	p, _ := pairing.Initiate(user.ID, "desktop-001", "TestDesk", nil, "")

	// Should fail — pairing not completed
	err := pairing.UploadConfig(p.PairingID, "data", "nonce")
	if err == nil {
		t.Error("expected error for incomplete pairing upload")
	}
}

func TestListDevices(t *testing.T) {
	db := testDB(t)
	auth := NewAuthService(db, testCfg())
	pairing := NewPairingService(db, testCfg())

	user, _ := auth.Register("test@example.com", "password123")
	p1, _ := pairing.Initiate(user.ID, "desktop-001", "TestDesk", nil, "")
	pairing.Complete(user.ID, p1.PairingID, "key1", "mobile-001", "TestPhone")
	p2, _ := pairing.Initiate(user.ID, "desktop-001", "TestDesk", nil, "")
	pairing.Complete(user.ID, p2.PairingID, "key2", "mobile-002", "TestPhone2")
	// Note: pending pairings are NOT listed as devices (only completed ones are)

	devices, err := pairing.ListDevices(user.ID, "", "", "")
	if err != nil {
		t.Fatalf("list failed: %v", err)
	}
	// Should include only completed, exclude pending and revoked
	if len(devices) != 2 {
		t.Errorf("expected 2 completed devices, got %d", len(devices))
	}

	// Revoke one and check it's excluded
	pairing.Revoke(p1.PairingID, user.ID)
	devices, _ = pairing.ListDevices(user.ID, "", "", "")
	if len(devices) != 1 {
		t.Errorf("expected 1 device after revoke, got %d", len(devices))
	}
}

func TestRefreshPairingJWT(t *testing.T) {
	db := testDB(t)
	auth := NewAuthService(db, testCfg())
	pairing := NewPairingService(db, testCfg())

	user, _ := auth.Register("test@example.com", "password123")
	p, _ := pairing.Initiate(user.ID, "desktop-001", "TestDesk", nil, "")
	pairing.Complete(user.ID, p.PairingID, "pubkey", "mobile-001", "TestPhone")

	// Refresh should work
	jwt, err := pairing.RefreshPairingJWT(p.PairingID)
	if err != nil {
		t.Fatalf("refresh failed: %v", err)
	}
	if jwt == "" {
		t.Error("expected non-empty jwt")
	}

	// Validate it's a tunnel-scope JWT
	claims, err := auth.ValidateJWT(jwt)
	if err != nil {
		t.Fatalf("validate failed: %v", err)
	}
	scope, _ := claims["scope"].(string)
	if scope != "tunnel" {
		t.Errorf("expected scope tunnel, got %s", scope)
	}
	pid, _ := claims["pairing_id"].(string)
	if pid != p.PairingID {
		t.Errorf("expected pairing_id %s, got %s", p.PairingID, pid)
	}
}

func TestPairingNotFound(t *testing.T) {
	db := testDB(t)
	pairing := NewPairingService(db, testCfg())

	_, err := pairing.Status("nonexistent")
	if err == nil {
		t.Error("expected error for non-existent pairing")
	}

	_, _, err = pairing.Complete(0, "nonexistent", "key", "dev", "TestPhone")
	if err == nil {
		t.Error("expected error for non-existent pairing complete")
	}
}

// B2: InitiateDesktop should not clean up pending mobile pairings.
func TestInitiateDesktopDoesNotCleanMobilePending(t *testing.T) {
	db := testDB(t)
	auth := NewAuthService(db, testCfg())
	pairing := NewPairingService(db, testCfg())

	userA, _ := auth.Register("a@example.com", "pass")
	userB, _ := auth.Register("b@example.com", "pass")
	userP, _ := auth.Register("phone@example.com", "pass") // phone (initiator)

	// Create completed mobile pairings: phone ↔ userA, phone ↔ userB
	pmA, _ := pairing.Initiate(userA.ID, "desktop-A", "DeskA", nil, "")
	pairing.Complete(userP.ID, pmA.PairingID, "pubkey", "phone-001", "Phone")
	pmB, _ := pairing.Initiate(userB.ID, "desktop-B", "DeskB", nil, "")
	pairing.Complete(userP.ID, pmB.PairingID, "pubkey", "phone-001", "Phone")

	// Create a pending mobile pairing for userB (separate from the completed one)
	pm, _ := pairing.Initiate(userB.ID, "desktop-B2", "DeskB2", nil, "")
	if pm.Status != "pending" {
		t.Fatalf("expected pending, got %s", pm.Status)
	}

	// Now initiate a desktop pairing — should NOT delete the pending mobile pairing
	_, err := pairing.InitiateDesktop(userP.ID, userB.ID, "desktop-B", "DeskB", userA.ID, "desktop-A", "DeskA", "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2")
	if err != nil {
		t.Fatalf("initiate desktop failed: %v", err)
	}

	// The pending mobile pairing should still exist
	status, err := pairing.Status(pm.PairingID)
	if err != nil {
		t.Fatalf("mobile pairing was deleted: %v", err)
	}
	if status.Status != "pending" {
		t.Errorf("expected pending, got %s", status.Status)
	}
}

// B3: Desktop A (client) can find its own desktop pairings via ListDevices.
func TestListDevicesClientCanFindOwnDesktopPairing(t *testing.T) {
	db := testDB(t)
	auth := NewAuthService(db, testCfg())
	pairing := NewPairingService(db, testCfg())

	userA, _ := auth.Register("a@example.com", "pass") // client
	userB, _ := auth.Register("b@example.com", "pass") // server
	userP, _ := auth.Register("p@example.com", "pass") // phone (initiator)

	// Create completed mobile pairings for the initiator
	pmA, _ := pairing.Initiate(userA.ID, "desktop-A", "DeskA", nil, "")
	pairing.Complete(userP.ID, pmA.PairingID, "pubkey", "phone-001", "Phone")
	pmB, _ := pairing.Initiate(userB.ID, "desktop-B", "DeskB", nil, "")
	pairing.Complete(userP.ID, pmB.PairingID, "pubkey", "phone-001", "Phone")

	// Create a desktop pairing: B is server, A is client, P is initiator
	p, _ := pairing.InitiateDesktop(userP.ID, userB.ID, "desktop-B", "DeskB", userA.ID, "desktop-A", "DeskA", "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2")
	pairing.Complete(userP.ID, p.PairingID, "", "desktop-A", "DeskA")

	// Desktop A (client) should find this pairing
	devices, err := pairing.ListDevices(userA.ID, "", "", "desktop")
	if err != nil {
		t.Fatalf("list failed: %v", err)
	}
	if len(devices) != 1 {
		t.Fatalf("expected 1 desktop pairing for client A, got %d", len(devices))
	}
	if devices[0].PairingID != p.PairingID {
		t.Errorf("wrong pairing returned")
	}

	// Desktop B (server) should also find this pairing
	devices, _ = pairing.ListDevices(userB.ID, "", "", "desktop")
	if len(devices) != 1 {
		t.Errorf("expected 1 desktop pairing for server B, got %d", len(devices))
	}

	// Phone (initiator) should also find this pairing
	devices, _ = pairing.ListDevices(userP.ID, "", "", "desktop")
	if len(devices) != 1 {
		t.Errorf("expected 1 desktop pairing for initiator P, got %d", len(devices))
	}

	// Filter by mobile_device_id (client device ID) should work for A
	devices, _ = pairing.ListDevices(userA.ID, "", "desktop-A", "desktop")
	if len(devices) != 1 {
		t.Errorf("expected 1 device with mobile_device_id filter, got %d", len(devices))
	}
}

// B3: Revoke should work for both server and client.
func TestRevokeByClientUser(t *testing.T) {
	db := testDB(t)
	auth := NewAuthService(db, testCfg())
	pairing := NewPairingService(db, testCfg())

	userA, _ := auth.Register("a@example.com", "pass") // client
	userB, _ := auth.Register("b@example.com", "pass") // server
	userP, _ := auth.Register("p@example.com", "pass") // phone (initiator)

	// Create completed mobile pairings for the initiator
	pmA, _ := pairing.Initiate(userA.ID, "desktop-A", "DeskA", nil, "")
	pairing.Complete(userP.ID, pmA.PairingID, "pubkey", "phone-001", "Phone")
	pmB, _ := pairing.Initiate(userB.ID, "desktop-B", "DeskB", nil, "")
	pairing.Complete(userP.ID, pmB.PairingID, "pubkey", "phone-001", "Phone")

	p, _ := pairing.InitiateDesktop(userP.ID, userB.ID, "desktop-B", "DeskB", userA.ID, "desktop-A", "DeskA", "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2")
	pairing.Complete(userP.ID, p.PairingID, "", "desktop-A", "DeskA")

	// Client (A) should be able to revoke
	err := pairing.Revoke(p.PairingID, userA.ID)
	if err != nil {
		t.Fatalf("client revoke failed: %v", err)
	}
	status, _ := pairing.Status(p.PairingID)
	if status.Status != "revoked" {
		t.Errorf("expected revoked, got %s", status.Status)
	}
}

// Security: initiator can revoke desktop pairing
func TestRevokeByInitiator(t *testing.T) {
	db := testDB(t)
	auth := NewAuthService(db, testCfg())
	pairing := NewPairingService(db, testCfg())

	userA, _ := auth.Register("a@example.com", "pass")
	userB, _ := auth.Register("b@example.com", "pass")
	userP, _ := auth.Register("p@example.com", "pass")

	pmA, _ := pairing.Initiate(userA.ID, "desktop-A", "DeskA", nil, "")
	pairing.Complete(userP.ID, pmA.PairingID, "pubkey", "phone-001", "Phone")
	pmB, _ := pairing.Initiate(userB.ID, "desktop-B", "DeskB", nil, "")
	pairing.Complete(userP.ID, pmB.PairingID, "pubkey", "phone-001", "Phone")

	p, _ := pairing.InitiateDesktop(userP.ID, userB.ID, "desktop-B", "DeskB", userA.ID, "desktop-A", "DeskA", "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2")
	pairing.Complete(userP.ID, p.PairingID, "", "desktop-A", "DeskA")

	err := pairing.Revoke(p.PairingID, userP.ID)
	if err != nil {
		t.Fatalf("initiator revoke failed: %v", err)
	}
}

// Security: InitiateDesktop rejects if initiator has no mobile pairing with desktops
func TestInitiateDesktopRejectsNoMobilePairing(t *testing.T) {
	db := testDB(t)
	auth := NewAuthService(db, testCfg())
	pairing := NewPairingService(db, testCfg())

	userA, _ := auth.Register("a@example.com", "pass")
	userB, _ := auth.Register("b@example.com", "pass")
	userP, _ := auth.Register("p@example.com", "pass")

	// No mobile pairings created — should fail
	_, err := pairing.InitiateDesktop(userP.ID, userB.ID, "desktop-B", "DeskB", userA.ID, "desktop-A", "DeskA", "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2")
	if err == nil {
		t.Error("expected error for initiator without mobile pairings")
	}
}

// Security: Complete rejects unauthorized caller for desktop pairing
func TestCompleteRejectsUnauthorizedCaller(t *testing.T) {
	db := testDB(t)
	auth := NewAuthService(db, testCfg())
	pairing := NewPairingService(db, testCfg())

	userA, _ := auth.Register("a@example.com", "pass")
	userB, _ := auth.Register("b@example.com", "pass")
	userP, _ := auth.Register("p@example.com", "pass")
	userX, _ := auth.Register("x@example.com", "pass") // unauthorized

	pmA, _ := pairing.Initiate(userA.ID, "desktop-A", "DeskA", nil, "")
	pairing.Complete(userP.ID, pmA.PairingID, "pubkey", "phone-001", "Phone")
	pmB, _ := pairing.Initiate(userB.ID, "desktop-B", "DeskB", nil, "")
	pairing.Complete(userP.ID, pmB.PairingID, "pubkey", "phone-001", "Phone")

	p, _ := pairing.InitiateDesktop(userP.ID, userB.ID, "desktop-B", "DeskB", userA.ID, "desktop-A", "DeskA", "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2")

	// UserX (unrelated) should not be able to complete
	_, _, err := pairing.Complete(userX.ID, p.PairingID, "", "desktop-A", "DeskA")
	if err == nil {
		t.Error("expected error for unauthorized caller")
	}
}

// B7: Complete's revoke logic should not cross pairing types.
func TestCompleteDoesNotRevokeAcrossTypes(t *testing.T) {
	db := testDB(t)
	auth := NewAuthService(db, testCfg())
	pairing := NewPairingService(db, testCfg())

	userA, _ := auth.Register("a@example.com", "pass")
	userB, _ := auth.Register("b@example.com", "pass")
	userP, _ := auth.Register("p@example.com", "pass")

	// Create completed mobile pairings for the initiator
	pmA, _ := pairing.Initiate(userA.ID, "desktop-A", "DeskA", nil, "")
	pairing.Complete(userP.ID, pmA.PairingID, "pubkey", "phone-001", "Phone")
	pmB, _ := pairing.Initiate(userB.ID, "desktop-B", "DeskB", nil, "")
	pairing.Complete(userP.ID, pmB.PairingID, "pubkey", "phone-001", "Phone")

	// Create a mobile pairing with device_id "dev-X"
	pm, _ := pairing.Initiate(userB.ID, "desktop-B", "DeskB", nil, "")
	pairing.Complete(userP.ID, pm.PairingID, "pubkey", "dev-X", "PhoneX")

	// Now create a desktop pairing with the SAME mobile_device_id "dev-X"
	pd, _ := pairing.InitiateDesktop(userP.ID, userB.ID, "desktop-B", "DeskB", userA.ID, "dev-X", "DeskA", "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2")
	pairing.Complete(userP.ID, pd.PairingID, "", "dev-X", "DeskA")

	// The mobile pairing should NOT be revoked (different pairing_type)
	status, _ := pairing.Status(pm.PairingID)
	if status.Status != "completed" {
		t.Errorf("mobile pairing should still be completed, got %s", status.Status)
	}

	// The desktop pairing should be completed
	status, _ = pairing.Status(pd.PairingID)
	if status.Status != "completed" {
		t.Errorf("desktop pairing should be completed, got %s", status.Status)
	}
}

// B4: Complete with empty phone_pubkey should work for desktop pairings.
func TestCompleteWithEmptyPubkey(t *testing.T) {
	db := testDB(t)
	auth := NewAuthService(db, testCfg())
	pairing := NewPairingService(db, testCfg())

	userA, _ := auth.Register("a@example.com", "pass")
	userB, _ := auth.Register("b@example.com", "pass")
	userP, _ := auth.Register("p@example.com", "pass")

	pmA, _ := pairing.Initiate(userA.ID, "desktop-A", "DeskA", nil, "")
	pairing.Complete(userP.ID, pmA.PairingID, "pubkey", "phone-001", "Phone")
	pmB, _ := pairing.Initiate(userB.ID, "desktop-B", "DeskB", nil, "")
	pairing.Complete(userP.ID, pmB.PairingID, "pubkey", "phone-001", "Phone")

	p, _ := pairing.InitiateDesktop(userP.ID, userB.ID, "desktop-B", "DeskB", userA.ID, "desktop-A", "DeskA", "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2")
	jwt, refresh, err := pairing.Complete(userP.ID, p.PairingID, "", "desktop-A", "DeskA")
	if err != nil {
		t.Fatalf("complete with empty pubkey failed: %v", err)
	}
	if jwt == "" || refresh == "" {
		t.Error("expected non-empty tokens")
	}
}

// TestCompleteWithTrustLevel tests D5: trust_level is stored on the pairing.
func TestCompleteWithTrustLevel(t *testing.T) {
	db := testDB(t)
	auth := NewAuthService(db, testCfg())
	pairing := NewPairingService(db, testCfg())

	user, _ := auth.Register("desktop@example.com", "pass")
	phone, _ := auth.Register("phone@example.com", "pass")

	pm, _ := pairing.Initiate(user.ID, "desktop-001", "Desk", nil, "")

	// Complete with trust_level=local_only
	jwt, refresh, err := pairing.CompleteWithTrustLevel(phone.ID, pm.PairingID, "pubkey", "phone-001", "Phone", "local_only")
	if err != nil {
		t.Fatalf("CompleteWithTrustLevel failed: %v", err)
	}
	if jwt == "" || refresh == "" {
		t.Error("expected non-empty tokens")
	}

	// Verify trust_level was stored
	var p model.Pairing
	db.Where("pairing_id = ?", pm.PairingID).First(&p)
	if p.TrustLevel != "local_only" {
		t.Errorf("expected trust_level=local_only, got %s", p.TrustLevel)
	}
}

// TestCompleteWithTrustLevel_Default tests D5: empty trust_level defaults to "full".
func TestCompleteWithTrustLevel_Default(t *testing.T) {
	db := testDB(t)
	auth := NewAuthService(db, testCfg())
	pairing := NewPairingService(db, testCfg())

	user, _ := auth.Register("desktop@example.com", "pass")
	phone, _ := auth.Register("phone@example.com", "pass")

	pm, _ := pairing.Initiate(user.ID, "desktop-001", "Desk", nil, "")

	_, _, err := pairing.CompleteWithTrustLevel(phone.ID, pm.PairingID, "pubkey", "phone-001", "Phone", "")
	if err != nil {
		t.Fatalf("CompleteWithTrustLevel failed: %v", err)
	}

	var p model.Pairing
	db.Where("pairing_id = ?", pm.PairingID).First(&p)
	if p.TrustLevel != "full" {
		t.Errorf("expected trust_level=full (default), got %s", p.TrustLevel)
	}
}

// TestCompleteWithTrustLevel_Invalid tests D5: invalid trust_level is rejected.
func TestCompleteWithTrustLevel_Invalid(t *testing.T) {
	db := testDB(t)
	auth := NewAuthService(db, testCfg())
	pairing := NewPairingService(db, testCfg())

	user, _ := auth.Register("desktop@example.com", "pass")
	phone, _ := auth.Register("phone@example.com", "pass")

	pm, _ := pairing.Initiate(user.ID, "desktop-001", "Desk", nil, "")

	_, _, err := pairing.CompleteWithTrustLevel(phone.ID, pm.PairingID, "pubkey", "phone-001", "Phone", "invalid")
	if err == nil {
		t.Error("expected error for invalid trust_level")
	}
}

// pairing_type filter in ListDevices
func TestListDevicesFilterByPairingType(t *testing.T) {
	db := testDB(t)
	auth := NewAuthService(db, testCfg())
	pairing := NewPairingService(db, testCfg())

	user, _ := auth.Register("test@example.com", "pass")
	userA, _ := auth.Register("a@example.com", "pass")
	userP, _ := auth.Register("p@example.com", "pass")

	// Mobile pairing (user is desktop owner, userP is phone user)
	pm, _ := pairing.Initiate(user.ID, "desktop-001", "Desk", nil, "")
	pairing.Complete(userP.ID, pm.PairingID, "pubkey", "mobile-001", "Phone")

	// Set up mobile pairings for initiator (userP) with both desktops
	pmA, _ := pairing.Initiate(userA.ID, "desktop-A", "DeskA", nil, "")
	pairing.Complete(userP.ID, pmA.PairingID, "pubkey", "phone-001", "Phone")
	pmB, _ := pairing.Initiate(user.ID, "desktop-002", "Desk2", nil, "")
	pairing.Complete(userP.ID, pmB.PairingID, "pubkey", "phone-001", "Phone")

	// Desktop pairing (user is server, userA is client, userP is initiator)
	pd, _ := pairing.InitiateDesktop(userP.ID, user.ID, "desktop-001", "Desk", userA.ID, "desktop-A", "DeskA", "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2")
	pairing.Complete(userP.ID, pd.PairingID, "", "desktop-A", "DeskA")

	// Filter mobile only — user has 2 mobile pairings (pm and pmB)
	devices, _ := pairing.ListDevices(user.ID, "", "", "mobile")
	if len(devices) != 2 {
		t.Errorf("expected 2 mobile pairings, got %d", len(devices))
	}

	// Filter desktop only — user is server of 1 desktop pairing
	devices, _ = pairing.ListDevices(user.ID, "", "", "desktop")
	if len(devices) != 1 {
		t.Errorf("expected 1 desktop pairing, got %d", len(devices))
	}

	// No filter — should return all (2 mobile + 1 desktop = 3)
	devices, _ = pairing.ListDevices(user.ID, "", "", "")
	if len(devices) != 3 {
		t.Errorf("expected 3 pairings, got %d", len(devices))
	}
}

// Security: InitiateDesktop must verify the initiator is the phone user of the mobile pairing,
// not just any user who knows the desktop user_ids.
func TestInitiateDesktopRejectsNonPhoneUser(t *testing.T) {
	db := testDB(t)
	auth := NewAuthService(db, testCfg())
	pairing := NewPairingService(db, testCfg())

	userA, _ := auth.Register("a@example.com", "pass")
	userB, _ := auth.Register("b@example.com", "pass")
	userP, _ := auth.Register("p@example.com", "pass") // actual phone user
	userX, _ := auth.Register("x@example.com", "pass") // attacker (different user)

	// Create completed mobile pairings: userP ↔ userA, userP ↔ userB
	pmA, _ := pairing.Initiate(userA.ID, "desktop-A", "DeskA", nil, "")
	pairing.Complete(userP.ID, pmA.PairingID, "pubkey", "phone-001", "Phone")
	pmB, _ := pairing.Initiate(userB.ID, "desktop-B", "DeskB", nil, "")
	pairing.Complete(userP.ID, pmB.PairingID, "pubkey", "phone-001", "Phone")

	// userX (attacker) tries to initiate desktop pairing between userA and userB.
	// userX knows the desktop user_ids but has no mobile pairing with either.
	_, err := pairing.InitiateDesktop(userX.ID, userB.ID, "desktop-B", "DeskB", userA.ID, "desktop-A", "DeskA", "a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2c3d4e5f6a1b2")
	if err == nil {
		t.Fatal("expected error: non-phone user should not be able to initiate desktop pairing")
	}
}
