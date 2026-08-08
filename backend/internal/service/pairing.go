package service

import (
	"errors"
	"fmt"
	"time"

	"github.com/golang-jwt/jwt/v5"
	"github.com/google/uuid"
	"github.com/termfast/backend/internal/config"
	"github.com/termfast/backend/internal/model"
	"gorm.io/gorm"
)

// PairingService handles pairing lifecycle and config sync.
type PairingService struct {
	db          *gorm.DB
	cfg         *config.Config
	nonceSvc    *DeviceKeyNonceService
	audit       *AuditService
}

// NewPairingService creates a new PairingService.
func NewPairingService(db *gorm.DB, cfg *config.Config) *PairingService {
	return &PairingService{db: db, cfg: cfg, nonceSvc: NewDeviceKeyNonceService(), audit: NewAuditService(db)}
}

// GetNonceService returns the nonce service for register-device-key (used by handler).
func (s *PairingService) GetNonceService() *DeviceKeyNonceService {
	return s.nonceSvc
}

// Initiate creates a pending pairing record.
// B6: Now accepts device_public_key and key_security_level for automatic
// DeviceKey registration at Complete time.
func (s *PairingService) Initiate(userID uint, desktopDeviceID, desktopName string, devicePublicKey []byte, keySecurityLevel string) (*model.Pairing, error) {
	// Clean up stale pending mobile pairings for this user to avoid clutter.
	// Only delete mobile pairings — desktop pairings in "pending" status belong
	// to approved JoinBatches and must not be deleted.
	s.db.Where("desktop_user_id = ? AND status = ? AND pairing_type = ?",
		userID, "pending", "mobile").Delete(&model.Pairing{})
	p := model.Pairing{
		PairingID:        uuid.New().String(),
		DesktopUserID:    userID,
		DesktopDeviceID:  desktopDeviceID,
		DesktopName:      desktopName,
		DevicePublicKey:  devicePublicKey,
		KeySecurityLevel: keySecurityLevel,
		PairingType:      "mobile",
		Status:           "pending",
	}
	if err := s.db.Create(&p).Error; err != nil {
		return nil, err
	}
	return &p, nil
}

// InitiateDesktop creates a pending desktop-to-desktop pairing record.
// initiatorUserID is the phone user who initiates the pairing.
// serverUserID is Desktop B (server), clientUserID is Desktop A (client).
// Security: verifies that the initiator has completed mobile pairings with
// both desktops before creating the desktop pairing.
// B2: Does NOT clean up pending pairings — desktop pairing is explicitly
// triggered by the phone, no stale-pending cleanup needed.
func (s *PairingService) InitiateDesktop(initiatorUserID uint, serverUserID uint, serverDeviceID, serverName string,
	clientUserID uint, clientDeviceID, clientName, pairingKeyHex string) (*model.Pairing, error) {
	// Security check: initiator must have completed mobile pairings with both desktops.
	// The mobile pairing must have mobile_user_id = initiatorUserID, proving the initiator
	// is the phone user who paired with these desktops (not just any user who knows the desktop user_ids).
	var count int64
	// Check initiator ↔ server (B) mobile pairing exists
	s.db.Model(&model.Pairing{}).
		Where("desktop_user_id = ? AND mobile_user_id = ? AND pairing_type = ? AND status = ?",
			serverUserID, initiatorUserID, "mobile", "completed").
		Count(&count)
	if count == 0 {
		return nil, errors.New("not authorized: no completed mobile pairing with server desktop")
	}
	// Check initiator ↔ client (A) mobile pairing exists
	s.db.Model(&model.Pairing{}).
		Where("desktop_user_id = ? AND mobile_user_id = ? AND pairing_type = ? AND status = ?",
			clientUserID, initiatorUserID, "mobile", "completed").
		Count(&count)
	if count == 0 {
		return nil, errors.New("not authorized: no completed mobile pairing with client desktop")
	}
	p := model.Pairing{
		PairingID:       uuid.New().String(),
		DesktopUserID:   serverUserID,
		ClientUserID:    clientUserID,
		InitiatorUserID: initiatorUserID,
		DesktopDeviceID: serverDeviceID,
		DesktopName:     serverName,
		MobileDeviceID:  clientDeviceID,
		MobileName:      clientName,
		PairingType:     "desktop",
		Status:          "pending",
		PairingKeyHex:   pairingKeyHex,
	}
	if err := s.db.Create(&p).Error; err != nil {
		return nil, err
	}
	return &p, nil
}

// Complete finalizes a pairing with the mobile's ECDH public key.
// callerUserID is the user calling the API (from JWT).
// Security: for desktop pairings, the caller must be the initiator or a participant.
// For mobile pairings, the pairing_id in the QR code serves as the authorization
// token — no caller identity check is needed (the phone user is not the desktop owner).
// Returns pairing JWT + refresh token.
func (s *PairingService) Complete(callerUserID uint, pairingID, mobilePubkey, mobileDeviceID, mobileName string) (string, string, error) {
	var p model.Pairing
	if err := s.db.Where("pairing_id = ?", pairingID).First(&p).Error; err != nil {
		return "", "", errors.New("pairing not found")
	}
	// Authorization check — only for desktop pairings
	if p.PairingType == "desktop" {
		if callerUserID == 0 {
			return "", "", errors.New("not authorized: JWT required for desktop pairing completion")
		}
		if p.InitiatorUserID != callerUserID && p.DesktopUserID != callerUserID && p.ClientUserID != callerUserID {
			return "", "", errors.New("not authorized to complete this pairing")
		}
	}
	if p.Status == "revoked" {
		return "", "", errors.New("pairing revoked")
	}
	if p.Status == "completed" {
		return "", "", errors.New("pairing already completed")
	}
	// B7: pending_approval status cannot be completed directly.
	// Desktop pairings created via JoinBatch must wait for batch approval (status=approved)
	// before they can be completed. This prevents completing a pairing before the
	// target members have signed their approval.
	if p.Status == "pending_approval" {
		return "", "", errors.New("pairing is pending_approval: batch must be approved before completion")
	}
	// Revoke any previous completed pairing for the same user + same device + same desktop + same type
	// to prevent duplicate device entries (same desktop re-pairing overwrites old).
	// B7: Added pairing_type filter to prevent cross-type false matches when
	// mobile_device_id is reused for desktop client device IDs.
	var existing []model.Pairing
	s.db.Where("desktop_user_id = ? AND mobile_device_id = ? AND desktop_device_id = ? AND pairing_type = ? AND status = ?",
		p.DesktopUserID, mobileDeviceID, p.DesktopDeviceID, p.PairingType, "completed").Find(&existing)
	now := time.Now()
	for i := range existing {
		existing[i].Status = "revoked"
		existing[i].RevokedAt = &now
		s.db.Save(&existing[i])
	}
	p.MobilePubkey = mobilePubkey
	// For desktop pairings (created via JoinBatch), MobileDeviceID already stores
	// the client desktop's device ID (set by makeJoinPairing/makeDesktopPairing).
	// Overwriting it with the phone's device ID would break network topology.
	// Only update MobileDeviceID/MobileName for mobile pairings.
	if p.PairingType == "mobile" {
		p.MobileDeviceID = mobileDeviceID
		p.MobileName = mobileName
	}
	// Record the mobile user's user_id for mobile pairings (used by InitiateDesktop permission check).
	// callerUserID comes from the phone user's JWT (passed via Complete handler).
	if p.PairingType == "mobile" && callerUserID != 0 {
		p.MobileUserID = callerUserID
	}
	p.Status = "completed"
	p.CompletedAt = &now
	if err := s.db.Save(&p).Error; err != nil {
		return "", "", err
	}
	// B6: Auto-register device public key to DeviceKey table (mobile pairings only).
	// This binds the key registration to the physical QR scan, preventing race attacks.
	// Handles both first-time registration and lost-key recovery (supersede old key).
	if p.PairingType == "mobile" && len(p.DevicePublicKey) > 0 {
		if err := s.registerDeviceKeyAuto(p.DesktopUserID, p.DesktopDeviceID, p.DevicePublicKey, p.KeySecurityLevel, now); err != nil {
			return "", "", fmt.Errorf("register device key: %w", err)
		}
	}
	// §6.6: If this pairing belongs to a JoinBatch, check if all sibling pairings
	// are completed. If so, mark the JoinBatch as completed.
	if p.JoinBatchID != "" {
		s.maybeMarkBatchCompleted(p.JoinBatchID)
	}
	jwt, err := s.issuePairingJWT(p.PairingID)
	if err != nil {
		return "", "", err
	}
	refresh, err := s.issuePairingRefreshToken(p.PairingID)
	if err != nil {
		return "", "", err
	}
	return jwt, refresh, nil
}

// maybeMarkBatchCompleted checks if all pairings in a JoinBatch are completed,
// and if so, marks the JoinBatch status as "completed".
// Design doc §6.6: "手机逐条调 Complete → 每条 Pairing.status = completed
// → 全部完成后 JoinBatch.status = completed"
func (s *PairingService) maybeMarkBatchCompleted(batchID string) {
	var batch model.JoinBatch
	if err := s.db.Where("batch_id = ?", batchID).First(&batch).Error; err != nil {
		return // batch not found, no-op
	}
	if batch.Status != "approved" {
		return // only transition from approved → completed
	}
	// Check if all sibling pairings are completed
	var pendingCount int64
	s.db.Model(&model.Pairing{}).
		Where("join_batch_id = ? AND status != ?", batchID, "completed").
		Count(&pendingCount)
	if pendingCount == 0 {
		// All pairings completed → mark batch as completed
		s.db.Model(&batch).Update("status", "completed")
		// B11: Audit log — batch_completed
		if s.audit != nil {
			s.audit.AuditEvent("batch_completed", batchID, batch.InitiatorUserID, "",
				0, "", map[string]interface{}{
					"flow_type": batch.FlowType,
				})
		}
	}
}

// CompleteWithTrustLevel is like Complete but also sets the trust_level field (D5).
// trustLevel must be "full" or "local_only"; empty defaults to "full".
func (s *PairingService) CompleteWithTrustLevel(callerUserID uint, pairingID, mobilePubkey, mobileDeviceID, mobileName, trustLevel string) (string, string, error) {
	if trustLevel == "" {
		trustLevel = "full"
	}
	// Validate trust_level
	if trustLevel != "full" && trustLevel != "local_only" {
		return "", "", errors.New("invalid trust_level: must be 'full' or 'local_only'")
	}
	var p model.Pairing
	if err := s.db.Where("pairing_id = ?", pairingID).First(&p).Error; err != nil {
		return "", "", errors.New("pairing not found")
	}
	// Authorization check — only for desktop pairings
	if p.PairingType == "desktop" {
		if callerUserID == 0 {
			return "", "", errors.New("not authorized: JWT required for desktop pairing completion")
		}
		if p.InitiatorUserID != callerUserID && p.DesktopUserID != callerUserID && p.ClientUserID != callerUserID {
			return "", "", errors.New("not authorized to complete this pairing")
		}
	}
	if p.Status == "revoked" {
		return "", "", errors.New("pairing revoked")
	}
	if p.Status == "completed" {
		return "", "", errors.New("pairing already completed")
	}
	if p.Status == "pending_approval" {
		return "", "", errors.New("pairing is pending_approval: batch must be approved before completion")
	}
	// Revoke any previous completed pairing for the same user + same device + same desktop + same type
	var existing []model.Pairing
	s.db.Where("desktop_user_id = ? AND mobile_device_id = ? AND desktop_device_id = ? AND pairing_type = ? AND status = ?",
		p.DesktopUserID, mobileDeviceID, p.DesktopDeviceID, p.PairingType, "completed").Find(&existing)
	now := time.Now()
	for i := range existing {
		existing[i].Status = "revoked"
		existing[i].RevokedAt = &now
		s.db.Save(&existing[i])
	}
	p.MobilePubkey = mobilePubkey
	// For desktop pairings (created via JoinBatch), MobileDeviceID already stores
	// the client desktop's device ID (set by makeJoinPairing/makeDesktopPairing).
	// Overwriting it with the phone's device ID would break network topology.
	// Only update MobileDeviceID/MobileName for mobile pairings.
	if p.PairingType == "mobile" {
		p.MobileDeviceID = mobileDeviceID
		p.MobileName = mobileName
	}
	p.TrustLevel = trustLevel
	if p.PairingType == "mobile" && callerUserID != 0 {
		p.MobileUserID = callerUserID
	}
	p.Status = "completed"
	p.CompletedAt = &now
	if err := s.db.Save(&p).Error; err != nil {
		return "", "", err
	}
	if p.PairingType == "mobile" && len(p.DevicePublicKey) > 0 {
		if err := s.registerDeviceKeyAuto(p.DesktopUserID, p.DesktopDeviceID, p.DevicePublicKey, p.KeySecurityLevel, now); err != nil {
			return "", "", fmt.Errorf("register device key: %w", err)
		}
	}
	// §6.6: If this pairing belongs to a JoinBatch, check if all sibling pairings
	// are completed. If so, mark the JoinBatch as completed.
	if p.JoinBatchID != "" {
		s.maybeMarkBatchCompleted(p.JoinBatchID)
	}
	jwt, err := s.issuePairingJWT(p.PairingID)
	if err != nil {
		return "", "", err
	}
	refresh, err := s.issuePairingRefreshToken(p.PairingID)
	if err != nil {
		return "", "", err
	}
	return jwt, refresh, nil
}

// Status returns the current pairing status.
func (s *PairingService) Status(pairingID string) (*model.Pairing, error) {
	var p model.Pairing
	if err := s.db.Where("pairing_id = ?", pairingID).First(&p).Error; err != nil {
		return nil, errors.New("pairing not found")
	}
	return &p, nil
}

// Revoke marks a pairing as revoked.
// The server (desktop_user_id), client (client_user_id), or initiator (initiator_user_id) can revoke.
func (s *PairingService) Revoke(pairingID string, userID uint) error {
	var p model.Pairing
	if err := s.db.Where("pairing_id = ?", pairingID).First(&p).Error; err != nil {
		return errors.New("pairing not found")
	}
	if p.DesktopUserID != userID && p.ClientUserID != userID && p.InitiatorUserID != userID {
		return errors.New("not authorized")
	}
	now := time.Now()
	p.Status = "revoked"
	p.RevokedAt = &now
	return s.db.Save(&p).Error
}

// ListDevices returns all pairings for a user, optionally filtered by
// desktop_device_id, mobile_device_id, and/or pairing_type.
// B3: Query condition changed to (desktop_user_id=? OR client_user_id=? OR initiator_user_id=?)
// so that Desktop A (client) can find its own desktop pairings, and the phone
// (initiator) can find the desktop pairings it initiated.
func (s *PairingService) ListDevices(userID uint, desktopDeviceID, mobileDeviceID, pairingType string) ([]model.Pairing, error) {
	var pairings []model.Pairing
	query := s.db.Where(
		"(desktop_user_id = ? OR client_user_id = ? OR initiator_user_id = ?) AND status = ?",
		userID, userID, userID, "completed",
	)
	if desktopDeviceID != "" {
		// For mobile pairings: desktop_device_id is the desktop's device ID.
		// For desktop pairings: desktop_device_id is server (B) device ID,
		//   mobile_device_id is client (A) device ID.
		// When the caller is the desktop itself, it may be either server (B)
		// or client (A), so match against both fields.
		// D9: device_id may have a 4-digit hex suffix appended (e.g. "host-user-a1b2").
		// Match exact, or DB value is a prefix of the query value (DB stored without suffix).
		query = query.Where(
			"(desktop_device_id = ? OR mobile_device_id = ? OR "+
				"? LIKE CONCAT(desktop_device_id, '-%') OR "+
				"? LIKE CONCAT(mobile_device_id, '-%'))",
			desktopDeviceID, desktopDeviceID, desktopDeviceID, desktopDeviceID,
		)
	}
	if mobileDeviceID != "" {
		query = query.Where("mobile_device_id = ?", mobileDeviceID)
	}
	if pairingType != "" {
		query = query.Where("pairing_type = ?", pairingType)
	}
	if err := query.Find(&pairings).Error; err != nil {
		return nil, err
	}
	return pairings, nil
}

// UploadConfig stores encrypted config ciphertext for a pairing.
func (s *PairingService) UploadConfig(pairingID, ciphertext, nonce string) error {
	var p model.Pairing
	if err := s.db.Where("pairing_id = ? AND status = ?", pairingID, "completed").First(&p).Error; err != nil {
		return errors.New("pairing not found or not completed")
	}
	cc := model.ConfigCiphertext{}
	result := s.db.Where("pairing_id = ?", pairingID).First(&cc)
	if result.Error != nil {
		// Create new
		cc = model.ConfigCiphertext{
			PairingID:  pairingID,
			Ciphertext: ciphertext,
			Nonce:      nonce,
		}
		return s.db.Create(&cc).Error
	}
	// Update existing
	cc.Ciphertext = ciphertext
	cc.Nonce = nonce
	return s.db.Save(&cc).Error
}

// DownloadConfig retrieves encrypted config for a pairing.
func (s *PairingService) DownloadConfig(pairingID string) (*model.ConfigCiphertext, error) {
	var p model.Pairing
	if err := s.db.Where("pairing_id = ? AND status = ?", pairingID, "completed").First(&p).Error; err != nil {
		return nil, errors.New("pairing not found or not completed")
	}
	var cc model.ConfigCiphertext
	if err := s.db.Where("pairing_id = ?", pairingID).First(&cc).Error; err != nil {
		return nil, errors.New("no config uploaded")
	}
	return &cc, nil
}

// RefreshPairingJWT issues a new pairing JWT if the pairing is still valid.
func (s *PairingService) RefreshPairingJWT(pairingID string) (string, error) {
	var p model.Pairing
	if err := s.db.Where("pairing_id = ?", pairingID).First(&p).Error; err != nil {
		return "", errors.New("pairing not found")
	}
	if p.Status == "revoked" {
		return "", errors.New("pairing revoked")
	}
	return s.issuePairingJWT(p.PairingID)
}

// issuePairingJWT creates a pairing-scoped JWT for tunnel access.
func (s *PairingService) issuePairingJWT(pairingID string) (string, error) {
	claims := jwt.MapClaims{
		"pairing_id": pairingID,
		"scope":      "tunnel",
		"exp":        time.Now().Add(time.Duration(s.cfg.PairingJWTExpHours) * time.Hour).Unix(),
		"iat":        time.Now().Unix(),
	}
	token := jwt.NewWithClaims(jwt.SigningMethodHS256, claims)
	return token.SignedString([]byte(s.cfg.JWTSecret))
}

// issuePairingRefreshToken creates a refresh token for pairing JWT.
func (s *PairingService) issuePairingRefreshToken(pairingID string) (string, error) {
	claims := jwt.MapClaims{
		"pairing_id": pairingID,
		"scope":      "pairing_refresh",
		"token_id":   uuid.New().String(),
		"exp":        time.Now().Add(time.Duration(s.cfg.RefreshExpDays) * 24 * time.Hour).Unix(),
		"iat":        time.Now().Unix(),
	}
	token := jwt.NewWithClaims(jwt.SigningMethodHS256, claims)
	return token.SignedString([]byte(s.cfg.JWTSecret))
}
