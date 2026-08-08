package handler

import (
	"encoding/base64"
	"log"
	"net/http"
	"strings"

	"github.com/gin-gonic/gin"
	jwtv5 "github.com/golang-jwt/jwt/v5"
	"github.com/gorilla/websocket"
	"github.com/termfast/backend/internal/middleware"
	"github.com/termfast/backend/internal/service"
)

// notifUpgrader is the WebSocket upgrader for the /notifications endpoint.
var notifUpgrader = websocket.Upgrader{
	CheckOrigin: func(r *http.Request) bool {
		return true // Allow all origins (desktop app connects directly)
	},
}

// PairingHandler handles pairing and config sync endpoints.
type PairingHandler struct {
	pairing   *service.PairingService
	auth      *service.AuthService
	joinBatch *service.JoinBatchService
}

// NewPairingHandler creates a new PairingHandler.
func NewPairingHandler(pairing *service.PairingService, auth *service.AuthService, joinBatch *service.JoinBatchService) *PairingHandler {
	return &PairingHandler{pairing: pairing, auth: auth, joinBatch: joinBatch}
}

// GetJoinBatchService returns the JoinBatchService (for testing).
func (h *PairingHandler) GetJoinBatchService() *service.JoinBatchService {
	return h.joinBatch
}

// RegisterRoutes registers pairing routes on the router.
func (h *PairingHandler) RegisterRoutes(r *gin.Engine) {
	// Pairing endpoints — require user JWT
	pair := r.Group("/pair")
	pair.Use(middleware.JWTAuth(h.auth))
	pair.Use(middleware.RequireScope("user"))
	{
		pair.POST("/initiate", middleware.RateLimit(20.0/60.0, 20), h.Initiate)
		pair.POST("/initiate-desktop", middleware.RateLimit(20.0/60.0, 20), h.InitiateDesktop)
		pair.POST("/register-device-key", middleware.RateLimit(10.0/60.0, 10), h.RegisterDeviceKey)
		pair.GET("/device-key-nonce", middleware.RateLimit(10.0/60.0, 10), h.GetDeviceKeyNonce)
		pair.GET("/status", middleware.RateLimit(60.0/60.0, 60), h.Status)
		pair.DELETE("/:id", middleware.RateLimit(20.0/60.0, 20), h.Revoke)
	}

	// JoinBatch endpoints — require user JWT (D4/D8: ApproveJoin + GetBatchInfo + M2: RequestJoin)
	join := r.Group("/join")
	join.Use(middleware.JWTAuth(h.auth))
	join.Use(middleware.RequireScope("user"))
	{
		join.POST("/request", middleware.RateLimit(10.0/60.0, 10), h.RequestJoin)
		join.POST("/approve", middleware.RateLimit(20.0/60.0, 20), h.ApproveJoin)
		join.GET("/batch-info", middleware.RateLimit(60.0/60.0, 60), h.GetBatchInfo)
	}

	// D8: Notification WebSocket — desktop connects to receive join_batch_pending notifications
	r.GET("/notifications", h.HandleNotifications)

	// Pair complete — no JWT auth (mobile calls this with pairing_id from QR)
	r.POST("/pair/complete", middleware.RateLimit(20.0/60.0, 20), h.Complete)

	// Refresh pairing JWT — uses refresh token, not JWT auth
	r.POST("/auth/refresh-pairing", middleware.RateLimit(30.0/60.0, 30), h.RefreshPairing)

	// Config sync — requires pairing JWT (scope=tunnel)
	sync := r.Group("/sync")
	sync.Use(middleware.JWTAuth(h.auth))
	sync.Use(middleware.RequireScope("tunnel"))
	{
		sync.POST("/config", middleware.RateLimit(30.0/60.0, 30), h.UploadConfig)
		sync.GET("/config", middleware.RateLimit(30.0/60.0, 30), h.DownloadConfig)
	}

	// Devices list — requires user JWT
	devices := r.Group("/devices")
	devices.Use(middleware.JWTAuth(h.auth))
	devices.Use(middleware.RequireScope("user"))
	{
		devices.GET("", h.ListDevices)
	}
}

// Initiate handles POST /pair/initiate.
func (h *PairingHandler) Initiate(c *gin.Context) {
	claims := c.MustGet("claims").(jwtv5.MapClaims)
	userIDFloat, ok := claims["user_id"].(float64)
	if !ok {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "invalid claims"})
		return
	}
	var req struct {
		DesktopDeviceID  string `json:"desktop_device_id"`
		DesktopName      string `json:"desktop_name"`
		DevicePublicKey  string `json:"device_public_key"`  // B6: base64 DER, optional (backward compat)
		KeySecurityLevel string `json:"key_security_level"` // B6: high/medium/low, optional
	}
	_ = c.ShouldBindJSON(&req)
	var pubKeyBytes []byte
	if req.DevicePublicKey != "" {
		decoded, err := base64.StdEncoding.DecodeString(req.DevicePublicKey)
		if err != nil {
			c.JSON(http.StatusBadRequest, gin.H{"error": "invalid device_public_key: base64 decode failed"})
			return
		}
		pubKeyBytes = decoded
	}
	p, err := h.pairing.Initiate(uint(userIDFloat), req.DesktopDeviceID, req.DesktopName, pubKeyBytes, req.KeySecurityLevel)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"pairing_id": p.PairingID})
}

// InitiateDesktop handles POST /pair/initiate-desktop.
// Creates a desktop-to-desktop pairing. The phone acts as intermediary,
// providing both server (B) and client (A) user IDs and device info.
// Security: the phone user (from JWT) is stored as initiator_user_id and
// the service layer verifies the initiator has mobile pairings with both desktops.
func (h *PairingHandler) InitiateDesktop(c *gin.Context) {
	claims := c.MustGet("claims").(jwtv5.MapClaims)
	userIDFloat, ok := claims["user_id"].(float64)
	if !ok {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "invalid claims"})
		return
	}
	initiatorUserID := uint(userIDFloat)
	var req struct {
		ServerUserID    uint   `json:"server_user_id" binding:"required"`
		ServerDeviceID  string `json:"server_device_id" binding:"required"`
		ServerName      string `json:"server_name"`
		ClientUserID    uint   `json:"client_user_id" binding:"required"`
		ClientDeviceID  string `json:"client_device_id" binding:"required"`
		ClientName      string `json:"client_name"`
		PairingKeyHex   string `json:"pairing_key_hex"`
	}
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	p, err := h.pairing.InitiateDesktop(
		initiatorUserID,
		req.ServerUserID, req.ServerDeviceID, req.ServerName,
		req.ClientUserID, req.ClientDeviceID, req.ClientName,
		req.PairingKeyHex,
	)
	if err != nil {
		// Return 403 for authorization errors, 500 for internal errors
		errMsg := err.Error()
		if strings.HasPrefix(errMsg, "not authorized") {
			c.JSON(http.StatusForbidden, gin.H{"error": errMsg})
		} else {
			c.JSON(http.StatusInternalServerError, gin.H{"error": errMsg})
		}
		return
	}
	c.JSON(http.StatusOK, gin.H{"pairing_id": p.PairingID})
}

// Complete handles POST /pair/complete.
// This endpoint is unauthenticated (no JWT middleware) — the pairing_id from
// the QR code serves as the authorization token for mobile pairings.
// For desktop pairings, the caller must pass a JWT via the Authorization header
// and must be the initiator, server, or client of the pairing.
func (h *PairingHandler) Complete(c *gin.Context) {
	var req struct {
		PairingID      string `json:"pairing_id" binding:"required"`
		PhonePubkey    string `json:"phone_pubkey"` // B4: not required — empty for desktop pairings
		DeviceID       string `json:"device_id" binding:"required"`
		MobileName     string `json:"mobile_name"`
		TrustLevel     string `json:"trust_level"` // D5: "full" (default) or "local_only"
	}
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	// D5: Validate trust_level if provided
	if req.TrustLevel != "" && req.TrustLevel != "full" && req.TrustLevel != "local_only" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "trust_level must be 'full' or 'local_only'"})
		return
	}
	if req.TrustLevel == "" {
		req.TrustLevel = "full"
	}
	// Try to extract caller userID from JWT (optional — only for desktop pairings)
	var callerUserID uint
	if authHeader := c.GetHeader("Authorization"); strings.HasPrefix(authHeader, "Bearer ") {
		tokenStr := strings.TrimPrefix(authHeader, "Bearer ")
		claims, err := h.auth.ValidateJWT(tokenStr)
		if err == nil {
			if userIDFloat, ok := claims["user_id"].(float64); ok {
				callerUserID = uint(userIDFloat)
			}
		}
	}
	jwt, refresh, err := h.pairing.CompleteWithTrustLevel(callerUserID, req.PairingID, req.PhonePubkey, req.DeviceID, req.MobileName, req.TrustLevel)
	if err != nil {
		errMsg := err.Error()
		if strings.HasPrefix(errMsg, "not authorized") {
			c.JSON(http.StatusForbidden, gin.H{"error": errMsg})
		} else {
			c.JSON(http.StatusBadRequest, gin.H{"error": errMsg})
		}
		return
	}
	c.JSON(http.StatusOK, gin.H{
		"status":        "completed",
		"pairing_jwt":   jwt,
		"refresh_token": refresh,
	})
}

// Status handles GET /pair/status.
func (h *PairingHandler) Status(c *gin.Context) {
	pairingID := c.Query("pairing_id")
	if pairingID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "pairing_id required"})
		return
	}
	p, err := h.pairing.Status(pairingID)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": err.Error()})
		return
	}
	resp := gin.H{"status": p.Status}
	if p.Status == "completed" {
		resp["phone_pubkey"] = p.MobilePubkey
		resp["device_id"] = p.MobileDeviceID
	}
	c.JSON(http.StatusOK, resp)
}

// Revoke handles DELETE /pair/:id.
func (h *PairingHandler) Revoke(c *gin.Context) {
	pairingID := c.Param("id")
	claims := c.MustGet("claims").(jwtv5.MapClaims)
	userIDFloat, ok := claims["user_id"].(float64)
	if !ok {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "invalid claims"})
		return
	}
	if err := h.pairing.Revoke(pairingID, uint(userIDFloat)); err != nil {
		c.JSON(http.StatusForbidden, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"status": "revoked"})
}

// UploadConfig handles POST /sync/config.
func (h *PairingHandler) UploadConfig(c *gin.Context) {
	claims := c.MustGet("claims").(jwtv5.MapClaims)
	pairingID, _ := claims["pairing_id"].(string)
	var req struct {
		Ciphertext string `json:"ciphertext" binding:"required"`
		Nonce      string `json:"nonce" binding:"required"`
	}
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	if err := h.pairing.UploadConfig(pairingID, req.Ciphertext, req.Nonce); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"status": "uploaded"})
}

// DownloadConfig handles GET /sync/config.
func (h *PairingHandler) DownloadConfig(c *gin.Context) {
	claims := c.MustGet("claims").(jwtv5.MapClaims)
	pairingID, _ := claims["pairing_id"].(string)
	cc, err := h.pairing.DownloadConfig(pairingID)
	if err != nil {
		c.JSON(http.StatusNotFound, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{
		"ciphertext": cc.Ciphertext,
		"nonce":      cc.Nonce,
	})
}

// RefreshPairing handles POST /auth/refresh-pairing.
func (h *PairingHandler) RefreshPairing(c *gin.Context) {
	var req struct {
		RefreshToken string `json:"refresh_token" binding:"required"`
	}
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	claims, err := h.auth.ValidateJWT(req.RefreshToken)
	if err != nil {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "invalid refresh token"})
		return
	}
	scope, _ := claims["scope"].(string)
	if scope != "pairing_refresh" {
		c.JSON(http.StatusForbidden, gin.H{"error": "not a pairing refresh token"})
		return
	}
	pairingID, _ := claims["pairing_id"].(string)
	jwt, err := h.pairing.RefreshPairingJWT(pairingID)
	if err != nil {
		c.JSON(http.StatusForbidden, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"pairing_jwt": jwt, "token_type": "Bearer"})
}

// ListDevices handles GET /devices.
// Optional query params:
//   - desktop_device_id: filter to pairings initiated by that desktop
//   - mobile_device_id: filter to pairings for that mobile device
//   - pairing_type: filter to mobile or desktop pairings
// Without params, all user pairings are returned.
func (h *PairingHandler) ListDevices(c *gin.Context) {
	claims := c.MustGet("claims").(jwtv5.MapClaims)
	userIDFloat, ok := claims["user_id"].(float64)
	if !ok {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "invalid claims"})
		return
	}
	desktopDeviceID := c.Query("desktop_device_id")
	mobileDeviceID := c.Query("mobile_device_id")
	pairingType := c.Query("pairing_type")
	pairings, err := h.pairing.ListDevices(uint(userIDFloat), desktopDeviceID, mobileDeviceID, pairingType)
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": err.Error()})
		return
	}
	c.JSON(http.StatusOK, gin.H{"devices": pairings})
}

// RegisterDeviceKey handles POST /pair/register-device-key.
// B6: Replaces an existing device public key. Requires old private key signature
// (JWT alone is NOT sufficient — prevents T10 key injection attack).
func (h *PairingHandler) RegisterDeviceKey(c *gin.Context) {
	claims := c.MustGet("claims").(jwtv5.MapClaims)
	userIDFloat, ok := claims["user_id"].(float64)
	if !ok {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "invalid claims"})
		return
	}
	var req struct {
		DeviceID          string `json:"device_id" binding:"required"`
		NewPublicKey      string `json:"new_public_key" binding:"required"`      // base64 DER
		KeySecurityLevel  string `json:"key_security_level"`                     // high/medium/low
		ReplacementPayload string `json:"replacement_payload" binding:"required"` // base64 canonical JSON
		OldKeySignature   string `json:"old_key_signature" binding:"required"`   // base64 ECDSA DER
	}
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	newPubBytes, err := base64.StdEncoding.DecodeString(req.NewPublicKey)
	if err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": "invalid new_public_key: base64 decode failed"})
		return
	}
	err = h.pairing.RegisterDeviceKey(service.RegisterDeviceKeyInput{
		UserID:             uint(userIDFloat),
		DeviceID:           req.DeviceID,
		NewPublicKey:       newPubBytes,
		KeySecurityLevel:   req.KeySecurityLevel,
		ReplacementPayload: req.ReplacementPayload,
		OldKeySignature:    req.OldKeySignature,
	})
	if err != nil {
		errMsg := err.Error()
		if strings.HasPrefix(errMsg, "no active") || strings.HasPrefix(errMsg, "not authorized") || strings.Contains(errMsg, "signature verification failed") {
			c.JSON(http.StatusForbidden, gin.H{"error": errMsg})
		} else {
			c.JSON(http.StatusBadRequest, gin.H{"error": errMsg})
		}
		return
	}
	c.JSON(http.StatusOK, gin.H{"status": "registered"})
}

// GetDeviceKeyNonce handles GET /pair/device-key-nonce.
// Design doc §7.5: Step ① of two-step key replacement — get one-time server_nonce.
// The desktop includes this nonce in the replacement_payload before signing with old key.
func (h *PairingHandler) GetDeviceKeyNonce(c *gin.Context) {
	nonce, err := h.pairing.GetNonceService().GenerateNonce()
	if err != nil {
		c.JSON(http.StatusInternalServerError, gin.H{"error": "failed to generate nonce"})
		return
	}
	c.JSON(http.StatusOK, gin.H{"server_nonce": nonce})
}

// ApproveJoin handles POST /join/approve — D4: sign and submit approval.
// The approver signs the canonical JSON payload locally (D4), then submits
// the payload + signature to this endpoint. The backend verifies the signature
// and records the approval.
func (h *PairingHandler) ApproveJoin(c *gin.Context) {
	claims := c.MustGet("claims").(jwtv5.MapClaims)
	userIDFloat, ok := claims["user_id"].(float64)
	if !ok {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "invalid claims"})
		return
	}
	var req struct {
		BatchID          string `json:"batch_id" binding:"required"`
		ApproverDeviceID string `json:"approver_device_id" binding:"required"`
		Payload          string `json:"payload" binding:"required"`    // base64 canonical JSON
		Signature        string `json:"signature" binding:"required"`  // base64 ECDSA DER
	}
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}
	result, err := h.joinBatch.ApproveJoin(service.ApproveJoinInput{
		BatchID:          req.BatchID,
		ApproverUserID:   uint(userIDFloat),
		ApproverDeviceID: req.ApproverDeviceID,
		Payload:          req.Payload,
		Signature:        req.Signature,
	})
	if err != nil {
		errMsg := err.Error()
		if strings.Contains(errMsg, "not found") {
			c.JSON(http.StatusNotFound, gin.H{"error": errMsg})
		} else if strings.Contains(errMsg, "not authorized") ||
			strings.Contains(errMsg, "signature verification failed") ||
			strings.Contains(errMsg, "already signed") {
			c.JSON(http.StatusForbidden, gin.H{"error": errMsg})
		} else {
			c.JSON(http.StatusBadRequest, gin.H{"error": errMsg})
		}
		return
	}
	c.JSON(http.StatusOK, gin.H{
		"batch_id":           result.BatchID,
		"received_approvals": result.ReceivedApprovals,
		"required_approvals": result.RequiredApprovals,
		"status":             result.Status,
	})
}

// GetBatchInfo handles GET /join/batch-info?batch_id=... — D8: fetch batch details
// for the approval dialog. Returns target members, nonce, content hash, etc.
func (h *PairingHandler) GetBatchInfo(c *gin.Context) {
	claims := c.MustGet("claims").(jwtv5.MapClaims)
	userIDFloat, ok := claims["user_id"].(float64)
	if !ok {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "invalid claims"})
		return
	}
	batchID := c.Query("batch_id")
	if batchID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "batch_id is required"})
		return
	}
	deviceID := c.Query("device_id")
	if deviceID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "device_id is required"})
		return
	}
	info, err := h.joinBatch.GetBatchInfo(batchID, uint(userIDFloat), deviceID)
	if err != nil {
		errMsg := err.Error()
		if strings.Contains(errMsg, "not found") {
			c.JSON(http.StatusNotFound, gin.H{"error": errMsg})
		} else if strings.Contains(errMsg, "not authorized") {
			c.JSON(http.StatusForbidden, gin.H{"error": errMsg})
		} else {
			c.JSON(http.StatusBadRequest, gin.H{"error": errMsg})
		}
		return
	}
	c.JSON(http.StatusOK, info)
}

// RequestJoin handles POST /join/request — M2: mobile initiates a join network request.
// The mobile user selects two desktop devices to interconnect. The backend creates a JoinBatch
// and notifies the target desktop devices via WebSocket.
func (h *PairingHandler) RequestJoin(c *gin.Context) {
	claims := c.MustGet("claims").(jwtv5.MapClaims)
	userIDFloat, ok := claims["user_id"].(float64)
	if !ok {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "invalid claims"})
		return
	}
	initiatorUserID := uint(userIDFloat)

	var req struct {
		DesktopAUserID   uint   `json:"desktop_a_user_id" binding:"required"`
		DesktopADeviceID string `json:"desktop_a_device_id" binding:"required"`
		DesktopAName     string `json:"desktop_a_name"`
		DesktopBUserID   uint   `json:"desktop_b_user_id" binding:"required"`
		DesktopBDeviceID string `json:"desktop_b_device_id" binding:"required"`
		DesktopBName     string `json:"desktop_b_name"`
	}
	if err := c.ShouldBindJSON(&req); err != nil {
		c.JSON(http.StatusBadRequest, gin.H{"error": err.Error()})
		return
	}

	result, err := h.joinBatch.RequestJoinNetwork(service.RequestJoinNetworkInput{
		InitiatorUserID:   initiatorUserID,
		DesktopAUserID:    req.DesktopAUserID,
		DesktopADeviceID:  req.DesktopADeviceID,
		DesktopAName:      req.DesktopAName,
		DesktopBUserID:    req.DesktopBUserID,
		DesktopBDeviceID:  req.DesktopBDeviceID,
		DesktopBName:      req.DesktopBName,
	})
	if err != nil {
		errMsg := err.Error()
		if strings.Contains(errMsg, "not found") || strings.Contains(errMsg, "no pairing") {
			c.JSON(http.StatusNotFound, gin.H{"error": errMsg})
		} else if strings.Contains(errMsg, "already exists") || strings.Contains(errMsg, "conflict") {
			c.JSON(http.StatusConflict, gin.H{"error": errMsg})
		} else {
			c.JSON(http.StatusInternalServerError, gin.H{"error": errMsg})
		}
		return
	}

	c.JSON(http.StatusOK, gin.H{
		"batch_id":           result.BatchID,
		"flow_type":          result.FlowType,
		"required_approvals": result.RequiredApprovals,
		"target_members":     result.TargetMembers,
		"expires_at":         result.ExpiresAt,
	})
}

// HandleNotifications handles GET /notifications — D8: WebSocket for desktop notification listening.
// Desktop connects with user JWT (via query param ?token=...) and device_id (?device_id=...).
// The server registers the connection with NotificationService and delivers join_batch_pending
// notifications in real-time. Pending notifications are delivered on connect.
func (h *PairingHandler) HandleNotifications(c *gin.Context) {
	// 1. Extract JWT from query param (WebSocket cannot set Authorization header in browser)
	token := c.Query("token")
	if token == "" {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "missing token"})
		return
	}

	// 2. Validate JWT
	claims, err := h.auth.ValidateJWT(token)
	if err != nil {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "invalid token"})
		return
	}
	userIDFloat, ok := claims["user_id"].(float64)
	if !ok {
		c.JSON(http.StatusUnauthorized, gin.H{"error": "invalid claims"})
		return
	}
	userID := uint(userIDFloat)

	// 3. Extract device_id from query param
	deviceID := c.Query("device_id")
	if deviceID == "" {
		c.JSON(http.StatusBadRequest, gin.H{"error": "device_id is required"})
		return
	}

	// 4. Upgrade to WebSocket
	ws, err := notifUpgrader.Upgrade(c.Writer, c.Request, nil)
	if err != nil {
		log.Printf("notifications: ws upgrade error: %v", err)
		return
	}
	defer ws.Close()

	// 5. Register connection with NotificationService
	ns := h.joinBatch.GetNotificationService()
	ns.RegisterConnection(userID, deviceID, ws)
	defer ns.UnregisterConnection(userID, deviceID, ws)

	// 6. Send connected acknowledgment
	ws.WriteMessage(websocket.TextMessage, []byte(`{"type":"connected"}`))

	// 7. Read loop — keep connection alive, handle ping/pong
	for {
		_, _, err := ws.ReadMessage()
		if err != nil {
			break // connection closed
		}
	}
}
