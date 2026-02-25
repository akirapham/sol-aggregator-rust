#!/bin/bash
set -e

# AWS Deployment Script
# Builds, pushes to ECR, and deploys to ECS

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

echo "🚀 AWS Full Deploy Pipeline"
echo "==========================="

# Step 1: Build and Push
echo ""
echo "📦 Step 1/2: Building and pushing Docker image..."
cd "$PROJECT_ROOT"
bash "$SCRIPT_DIR/push.sh"

# Step 2: Deploy to ECS
echo ""
echo "🔄 Step 2/2: Deploying to ECS..."
aws ecs update-service \
    --cluster sol-agg-cluster \
    --service sol-agg-service \
    --force-new-deployment \
    --query "service.deployments[0].{Status:status,TaskDefinition:taskDefinition}" \
    --output table

echo ""
echo "✅ Deployment triggered!"
echo "   Monitor: aws ecs describe-services --cluster sol-agg-cluster --services sol-agg-service --query 'services[0].events[0:3]'"
echo "   Health:  curl http://sol-agg-alb-964909105.eu-central-1.elb.amazonaws.com/health"
