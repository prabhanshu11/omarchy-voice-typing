# Build stage
FROM golang:1.25-alpine AS builder

WORKDIR /app

# Copy go mod files
COPY go.mod ./
# COPY go.sum ./ # Not present yet as dependencies are not managed via module yet in this simple setup or vendor

# Copy source code
COPY . .

# Build the binary
RUN go build -o voice-gateway-gateway ./cmd/server

# Runtime stage
FROM alpine:latest

WORKDIR /app

# Install ca-certificates for HTTPS requests
RUN apk --no-cache add ca-certificates

# Copy binary from builder
COPY --from=builder /app/voice-gateway-gateway .

# Expose port
EXPOSE 8765

# Run the binary
ENTRYPOINT ["./voice-gateway-gateway"]