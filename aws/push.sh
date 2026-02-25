#!/bin/bash
set -e

# Configuration
AWS_REGION="eu-central-1"  # Change this if needed
ECR_REPO_NAME="solana-aggregator"
IMAGE_TAG="latest"

# 1. Get Account ID
ACCOUNT_ID=$(aws sts get-caller-identity --query Account --output text)
ECR_URI="${ACCOUNT_ID}.dkr.ecr.${AWS_REGION}.amazonaws.com"
FULL_IMAGE_URI="${ECR_URI}/${ECR_REPO_NAME}:${IMAGE_TAG}"

echo "🚀 Deploying to ECR: ${FULL_IMAGE_URI}..."

# 2. Login to ECR
echo "🔑 Logging in to ECR..."
aws ecr get-login-password --region ${AWS_REGION} | docker login --username AWS --password-stdin ${ECR_URI}

# 3. Build Docker Image
echo "🔨 Building Docker image for linux/amd64 (this may take a while)..."
# We use the root context (.) and specify the Dockerfile path
# --platform is critical for Mac M1/M2/M3 users to build x86_64 images for Fargate
docker build --platform linux/amd64 -t ${ECR_REPO_NAME}:${IMAGE_TAG} -f docker/Dockerfile.sol .

# 4. Tag Image
echo "🏷️ Tagging image..."
docker tag ${ECR_REPO_NAME}:${IMAGE_TAG} ${FULL_IMAGE_URI}

# 5. Push Image
echo "Rx_ Pushing image to ECR..."
docker push ${FULL_IMAGE_URI}

echo "✅ Success! Image available at: ${FULL_IMAGE_URI}"
echo "   You can now update your ECS Task Definition with this URI."
